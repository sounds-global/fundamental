//! The calls in a test source the isolation rules judge, each with the function it is in.

use std::collections::BTreeSet;

use proc_macro2::{Delimiter, TokenStream, TokenTree};
use syn::spanned::Spanned as _;
use syn::visit::{self, Visit};

/// The names a scan looks for.
#[derive(Debug, Clone, Default)]
pub(crate) struct Names {
    pub(crate) leasing: BTreeSet<String>,
    pub(crate) revoking: BTreeSet<String>,
    pub(crate) drains: BTreeSet<String>,
    pub(crate) confining: BTreeSet<String>,
    pub(crate) race_windows: BTreeSet<String>,
}

/// One call a rule judges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Found {
    /// A lease, with the names its result is bound to (`let (connections, _) = …`).
    Lease {
        call: String,
        bound: Vec<String>,
    },
    Drain {
        call: String,
        confined: bool,
    },
    Fixture {
        path: String,
    },
    /// A sleep for a fixed time, not for a named race window.
    Sleep {
        call: String,
    },
    /// The outcome of an `eventually` dropped (`let _ =`, `.ok()`), so the wait asserts nothing.
    DiscardedWait,
}

/// A call found, where it is: `body` tells apart two functions of one name in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Site {
    pub(crate) function: String,
    pub(crate) body: usize,
    pub(crate) line: usize,
    pub(crate) found: Found,
}

/// What a scan found: every judged call, and each name a revoking method was called on.
pub(crate) struct Scanned {
    pub(crate) sites: Vec<Site>,
    pub(crate) revoked: BTreeSet<(usize, String)>,
    /// `(body, alias, of)`: `if let Ok(alias) = &of` binds `alias` to what `of` holds.
    pub(crate) aliases: BTreeSet<(usize, String, String)>,
}

/// Every judged call in `file`, and every revoke.
pub(crate) fn scan(file: &syn::File, names: &Names) -> Scanned {
    let mut scan = FunctionScan {
        names,
        function: None,
        body: 0,
        bodies: 0,
        confining: 0,
        binding: Vec::new(),
        sites: Vec::new(),
        revoked: BTreeSet::new(),
        aliases: BTreeSet::new(),
    };
    scan.visit_file(file);
    Scanned {
        sites: scan.sites,
        revoked: scan.revoked,
        aliases: scan.aliases,
    }
}

struct FunctionScan<'a> {
    names: &'a Names,
    function: Option<String>,
    body: usize,
    bodies: usize,
    confining: usize,
    /// The names the `let` being initialised binds, while its initialiser is visited.
    binding: Vec<String>,
    sites: Vec<Site>,
    revoked: BTreeSet<(usize, String)>,
    aliases: BTreeSet<(usize, String, String)>,
}

impl FunctionScan<'_> {
    fn note(&mut self, line: usize, found: Found) {
        self.sites.push(Site {
            function: self.function.clone().unwrap_or_else(|| "?".to_owned()),
            body: self.body,
            line,
            found,
        });
    }

    fn within(&mut self, name: Option<String>, body: impl FnOnce(&mut Self)) {
        self.bodies += 1;
        let outer = std::mem::replace(&mut self.function, name);
        let outer_body = std::mem::replace(&mut self.body, self.bodies);
        let confining = std::mem::take(&mut self.confining);
        let binding = std::mem::take(&mut self.binding);
        body(self);
        self.function = outer;
        self.body = outer_body;
        self.confining = confining;
        self.binding = binding;
    }

    fn confined(&mut self, confines: bool, body: impl FnOnce(&mut Self)) {
        if confines {
            self.confining += 1;
        }
        body(self);
        if confines {
            self.confining -= 1;
        }
    }

    fn lease(&mut self, line: usize, call: String) {
        let bound = self.binding.clone();
        self.note(line, Found::Lease { call, bound });
    }
}

impl<'ast> Visit<'ast> for FunctionScan<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.within(Some(item.sig.ident.to_string()), |scan| {
            visit::visit_item_fn(scan, item);
        });
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.within(Some(item.sig.ident.to_string()), |scan| {
            visit::visit_impl_item_fn(scan, item);
        });
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        let syn::Expr::Path(path) = call.func.as_ref() else {
            visit::visit_expr_call(self, call);
            return;
        };
        let segments: Vec<String> = path
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect();
        let last = segments.last().cloned().unwrap_or_default();
        let qualified = match segments.as_slice() {
            [.., owner, name] => format!("{owner}::{name}"),
            _ => last.clone(),
        };
        let line = call.span().start().line;
        let names = self.names;
        // A registry built from settings leases its roles when it is handed a secrets store.
        let leases_from_settings = qualified == "SqlConnections::from_settings"
            && call.args.last().is_some_and(|arg| !is_none(arg));
        if names.leasing.contains(&last)
            || names.leasing.contains(&qualified)
            || leases_from_settings
        {
            self.lease(line, qualified.clone());
        }
        if matches!(last.as_str(), "sleep" | "sleep_until")
            && !call
                .args
                .first()
                .and_then(last_ident)
                .is_some_and(|name| names.race_windows.contains(&name))
        {
            self.note(
                line,
                Found::Sleep {
                    call: qualified.clone(),
                },
            );
        }
        if names.drains.contains(&last) || names.drains.contains(&qualified) {
            let confined = self.confining > 0;
            self.note(
                line,
                Found::Drain {
                    call: qualified,
                    confined,
                },
            );
        }
        let confines = names.confining.contains(&last);
        self.confined(confines, |scan| visit::visit_expr_call(scan, call));
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        let line = call.method.span().start().line;
        let names = self.names;
        if names.leasing.contains(&method) {
            self.lease(line, method.clone());
        }
        if names.revoking.contains(&method)
            && let Some(receiver) = named(&call.receiver)
        {
            self.revoked.insert((self.body, receiver));
        }
        if method == "ok" && waits(&call.receiver) {
            self.note(line, Found::DiscardedWait);
        }
        let dotted = format!(".{method}");
        if names.drains.contains(&dotted) {
            let confined = self.confining > 0;
            self.note(
                line,
                Found::Drain {
                    call: dotted,
                    confined,
                },
            );
        }
        let confines = names.confining.contains(&method);
        self.confined(confines, |scan| visit::visit_expr_method_call(scan, call));
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        if matches!(local.pat, syn::Pat::Wild(_))
            && let Some(init) = &local.init
            && waits(&init.expr)
        {
            self.note(local.span().start().line, Found::DiscardedWait);
        }
        let mut names = Vec::new();
        bound_names(&local.pat, &mut names);
        let outer = std::mem::replace(&mut self.binding, names);
        visit::visit_local(self, local);
        self.binding = outer;
    }

    fn visit_expr_let(&mut self, binding: &'ast syn::ExprLet) {
        if let Some(of) = named(&binding.expr) {
            let mut names = Vec::new();
            bound_names(&binding.pat, &mut names);
            for alias in names {
                self.aliases.insert((self.body, alias, of.clone()));
            }
        }
        visit::visit_expr_let(self, binding);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac.path.is_ident("include_str")
            && let Ok(path) = mac.parse_body::<syn::LitStr>()
            && path.value().ends_with(".sql")
        {
            self.note(
                mac.span().start().line,
                Found::Fixture { path: path.value() },
            );
        }
        // A macro's body is tokens, not syntax (`tokio::select!`), so a sleep in it is read there.
        for (line, call) in token_sleeps(mac.tokens.clone(), &self.names.race_windows) {
            self.note(line, Found::Sleep { call });
        }
        visit::visit_macro(self, mac);
    }
}

/// Every `sleep(..)` or `sleep_until(..)` in `tokens` that waits for none of `race_windows`.
fn token_sleeps(tokens: TokenStream, race_windows: &BTreeSet<String>) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let tokens: Vec<TokenTree> = tokens.into_iter().collect();
    for (at, token) in tokens.iter().enumerate() {
        match token {
            TokenTree::Group(group) => found.extend(token_sleeps(group.stream(), race_windows)),
            TokenTree::Ident(ident) if ident == "sleep" || ident == "sleep_until" => {
                let Some(TokenTree::Group(args)) = tokens.get(at + 1) else {
                    continue;
                };
                if args.delimiter() != Delimiter::Parenthesis
                    || args.stream().into_iter().any(|arg| {
                        matches!(&arg, TokenTree::Ident(name) if race_windows.contains(&name.to_string()))
                    })
                {
                    continue;
                }
                let owner = match (tokens.get(at.wrapping_sub(3)), at >= 3) {
                    (Some(TokenTree::Ident(owner)), true) => format!("{owner}::{ident}"),
                    _ => ident.to_string(),
                };
                found.push((ident.span().start().line, owner));
            }
            _ => {}
        }
    }
    found
}

/// The name `expr` reads, through references and parentheses: `connections`, `&lease`.
fn named(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(path) => path.path.get_ident().map(ToString::to_string),
        syn::Expr::Reference(reference) => named(&reference.expr),
        syn::Expr::Paren(paren) => named(&paren.expr),
        _ => None,
    }
}

/// Every name `pat` binds, but `_`.
fn bound_names(pat: &syn::Pat, names: &mut Vec<String>) {
    match pat {
        syn::Pat::Ident(ident) => names.push(ident.ident.to_string()),
        syn::Pat::Tuple(tuple) => tuple.elems.iter().for_each(|pat| bound_names(pat, names)),
        syn::Pat::TupleStruct(tuple) => tuple.elems.iter().for_each(|pat| bound_names(pat, names)),
        syn::Pat::Type(typed) => bound_names(&typed.pat, names),
        syn::Pat::Reference(reference) => bound_names(&reference.pat, names),
        _ => {}
    }
}

/// The last segment of the path `expr` reads: `RACE_WINDOW` in `support::RACE_WINDOW`.
fn last_ident(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        syn::Expr::Reference(reference) => last_ident(&reference.expr),
        syn::Expr::Paren(paren) => last_ident(&paren.expr),
        _ => None,
    }
}

/// Whether `expr` is a call to `eventually`, awaited or not.
fn waits(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Await(awaited) => waits(&awaited.base),
        syn::Expr::Paren(paren) => waits(&paren.expr),
        syn::Expr::Call(call) => matches!(
            call.func.as_ref(),
            syn::Expr::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "eventually")
        ),
        _ => false,
    }
}

fn is_none(arg: &syn::Expr) -> bool {
    matches!(arg, syn::Expr::Path(path) if path.path.is_ident("None"))
}

#[cfg(test)]
mod tests {
    use super::{Found, Names, scan};

    fn names() -> Names {
        Names {
            leasing: ["open_steward".to_owned()].into(),
            revoking: ["close".to_owned()].into(),
            drains: ["Drain::new".to_owned(), ".reconcile".to_owned()].into(),
            confining: ["confined".to_owned(), "confined_to".to_owned()].into(),
            race_windows: ["RACE_WINDOW".to_owned()].into(),
        }
    }

    fn found(source: &str) -> Vec<(String, Found)> {
        let file = syn::parse_file(source).expect("Rust");
        scan(&file, &names())
            .sites
            .into_iter()
            .map(|site| (site.function, site.found))
            .collect()
    }

    #[test]
    fn a_drain_inside_a_confining_call_is_confined() {
        let sites = found(
            "fn a() { confined(Drain::<K>::new(c), team); Drain::new(c).serving(h).confined_to(o); Drain::new(c); x.reconcile(n); }",
        );
        let confined: Vec<bool> = sites
            .iter()
            .filter_map(|(_, found)| match found {
                Found::Drain { confined, .. } => Some(*confined),
                _ => None,
            })
            .collect();
        assert_eq!(confined, [true, true, false, false]);
    }

    #[test]
    fn each_call_names_the_function_it_is_in() {
        let sites = found(
            "fn outer() { let (s, _) = open_steward(&app); fn inner() { SqlConnections::from_settings(&s, e, Some(&v)); } } fn plain() { SqlConnections::from_settings(&s, e, None); }",
        );
        assert_eq!(
            sites,
            [
                (
                    "outer".to_owned(),
                    Found::Lease {
                        call: "open_steward".to_owned(),
                        bound: vec!["s".to_owned()],
                    }
                ),
                (
                    "inner".to_owned(),
                    Found::Lease {
                        call: "SqlConnections::from_settings".to_owned(),
                        bound: Vec::new(),
                    }
                ),
            ],
            "a registry with no secrets store leases nothing"
        );
    }

    #[test]
    fn a_revoke_names_its_receiver_and_its_body() {
        let file = syn::parse_file(
            "fn a() { c.close().await; (&lease).close(); ws.send(x); } fn a() { other.close(); }",
        )
        .expect("Rust");
        let revoked = scan(&file, &names()).revoked;
        assert_eq!(
            revoked,
            [
                (1, "c".to_owned()),
                (1, "lease".to_owned()),
                (2, "other".to_owned())
            ]
            .into()
        );
    }

    #[test]
    fn a_sleep_is_found_unless_it_waits_for_a_race_window() {
        let sites = found(
            "fn a() { tokio::time::sleep(Duration::from_secs(1)).await; std::thread::sleep(d); sleep(support::RACE_WINDOW).await; eventually(\"x\", w, tokio::time::sleep, p).await; }",
        );
        assert_eq!(
            sites,
            [
                (
                    "a".to_owned(),
                    Found::Sleep {
                        call: "time::sleep".to_owned()
                    }
                ),
                (
                    "a".to_owned(),
                    Found::Sleep {
                        call: "thread::sleep".to_owned()
                    }
                ),
            ],
            "a sleep handed to `eventually` is not a call"
        );
    }

    #[test]
    fn a_sleep_inside_a_macro_is_found() {
        let sites = found(
            "fn a() { tokio::select! { () = waited => { tokio::time::sleep(Duration::from_millis(500)).await; } () = run => {} } tokio::select! { () = sleep(RACE_WINDOW) => {} } }",
        );
        assert_eq!(
            sites,
            [(
                "a".to_owned(),
                Found::Sleep {
                    call: "time::sleep".to_owned()
                }
            )]
        );
    }

    #[test]
    fn a_dropped_wait_is_found_and_a_kept_one_is_not() {
        let sites = found(
            "fn a() { let _ = eventually(\"x\", w, s, p).await; eventually(\"x\", w, s, p).await.ok(); let held = eventually(\"x\", w, s, p).await; eventually(\"x\", w, s, p).await.unwrap(); }",
        );
        assert_eq!(
            sites,
            [
                ("a".to_owned(), Found::DiscardedWait),
                ("a".to_owned(), Found::DiscardedWait)
            ]
        );
    }

    #[test]
    fn an_included_sql_fixture_is_found() {
        let sites =
            found("fn a() { run(include_str!(\"fixtures/x.sql\")); include_str!(\"y.toml\"); }");
        assert_eq!(
            sites,
            [(
                "a".to_owned(),
                Found::Fixture {
                    path: "fixtures/x.sql".to_owned()
                }
            )]
        );
    }
}
