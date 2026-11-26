//! [`scan_view`]: what a view file says that a humble view may not.

use std::collections::BTreeMap;
use std::path::Path;

use proc_macro2::{Delimiter, Span, TokenStream, TokenTree};
use quote::ToTokens;
use syn::ext::IdentExt;
use syn::visit::{self, Visit};
use syn::{Attribute, File, Item, ItemExternCrate, ItemMod, ItemUse, Meta, UseTree};

use super::architecture_finding::ArchitectureFinding;
use super::file_rule::FileRule;

/// A crate a view may not name, keyed by the name source uses for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Forbidden {
    /// The name the view would write.
    pub(crate) ident: String,
    /// The ring the table places it in.
    pub(crate) ring: String,
}

/// Every finding in one parsed view file, each with the rule a pending entry would freeze.
///
/// A `#[cfg(test)] mod` is skipped whole; macro bodies are read as tokens.
pub(crate) fn scan_view(
    parsed: &File,
    at: &Path,
    forbidden: &BTreeMap<String, Forbidden>,
) -> Vec<(FileRule, ArchitectureFinding)> {
    let mut file = parsed.clone();
    file.items.retain(|item| !is_test_module(item));

    let mut scan = Scan {
        at,
        forbidden,
        findings: Vec::new(),
    };
    scan.visit_file(&file);
    scan.tokens(file.into_token_stream());
    scan.findings.sort_by_key(|(rule, line, _)| (*rule, *line));
    scan.findings
        .into_iter()
        .map(|(rule, _, finding)| (rule, finding))
        .collect()
}

struct Scan<'a> {
    at: &'a Path,
    forbidden: &'a BTreeMap<String, Forbidden>,
    findings: Vec<(FileRule, usize, ArchitectureFinding)>,
}

impl Scan<'_> {
    fn here(&self, span: Span) -> String {
        format!("{}:{}", self.at.display(), span.start().line)
    }

    fn push(&mut self, rule: FileRule, span: Span, finding: ArchitectureFinding) {
        self.findings.push((rule, span.start().line, finding));
    }

    fn cfg(&mut self, span: Span, what: &str) {
        let at = self.here(span);
        let what = what.to_owned();
        self.push(
            FileRule::Cfg,
            span,
            ArchitectureFinding::CfgInView { at, what },
        );
    }

    fn wait(&mut self, span: Span, what: &str) {
        let at = self.here(span);
        let what = what.to_owned();
        self.push(
            FileRule::Async,
            span,
            ArchitectureFinding::AsyncInView { at, what },
        );
    }

    fn hidden(&mut self, span: Span, what: &str) {
        let at = self.here(span);
        let what = what.to_owned();
        self.push(
            FileRule::HiddenSource,
            span,
            ArchitectureFinding::HiddenSource { at, what },
        );
    }

    fn named(&mut self, ident: &syn::Ident) {
        let name = ident.unraw().to_string();
        if let Some(found) = self.forbidden.get(&name) {
            let at = self.here(ident.span());
            let finding = ArchitectureFinding::AdapterInView {
                at,
                krate: found.ident.clone(),
                ring: found.ring.to_string(),
            };
            self.push(FileRule::Adapter, ident.span(), finding);
        }
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    fn tokens(&mut self, stream: TokenStream) {
        let trees: Vec<TokenTree> = stream.into_iter().collect();
        for (index, tree) in trees.iter().enumerate() {
            let next = trees.get(index + 1);
            match tree {
                TokenTree::Group(group) => {
                    self.tokens(group.stream());
                }
                TokenTree::Punct(punct) if punct.as_char() == '#' => {
                    let attribute = match (next, trees.get(index + 2)) {
                        (Some(TokenTree::Group(group)), _) => Some(group),
                        (Some(TokenTree::Punct(bang)), Some(TokenTree::Group(group)))
                            if bang.as_char() == '!' =>
                        {
                            Some(group)
                        }
                        _ => None,
                    };
                    if let Some(group) = attribute
                        && group.delimiter() == Delimiter::Bracket
                        && let Some(TokenTree::Ident(name)) = group.stream().into_iter().next()
                        && (name.unraw() == "cfg" || name.unraw() == "cfg_attr")
                    {
                        self.cfg(punct.span(), &format!("#[{name}]"));
                    }
                }
                TokenTree::Punct(punct) if punct.as_char() == '.' => {
                    if let Some(TokenTree::Ident(ident)) = next
                        && ident == "await"
                    {
                        self.wait(ident.span(), ".await");
                    }
                }
                TokenTree::Ident(ident) => {
                    let word = ident.unraw().to_string();
                    let bang = is_bang(&trees, index + 1);
                    if ident == "async" {
                        self.wait(ident.span(), "async");
                    } else if word == "cfg" && bang {
                        self.cfg(ident.span(), "cfg!");
                    } else if (word == "include" || word == "macro_rules") && bang {
                        self.hidden(ident.span(), &format!("{word}!"));
                    } else if self.forbidden.contains_key(&word)
                        && is_path_separator(&trees, index + 1)
                        && starts_a_path(&trees, index)
                    {
                        self.named(ident);
                    }
                }
                TokenTree::Punct(_) | TokenTree::Literal(_) => {}
            }
        }
    }
}

impl<'ast> Visit<'ast> for Scan<'_> {
    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        if item.content.is_none() {
            self.hidden(item.ident.span(), &format!("mod {};", item.ident));
        }
        visit::visit_item_mod(self, item);
    }

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        if attribute.path().is_ident("path") {
            self.hidden(attribute.pound_token.span, "#[path]");
        }
        visit::visit_attribute(self, attribute);
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        self.use_root(&item.tree);
    }

    fn visit_item_extern_crate(&mut self, item: &'ast ItemExternCrate) {
        self.named(&item.ident);
    }
}

impl Scan<'_> {
    /// A crate named at the root of a `use` with no path after it (`use x;`, `use x as y;`).
    fn use_root(&mut self, tree: &UseTree) {
        match tree {
            UseTree::Name(name) => self.named(&name.ident),
            UseTree::Rename(rename) => self.named(&rename.ident),
            UseTree::Group(group) => group.items.iter().for_each(|item| self.use_root(item)),
            UseTree::Path(_) | UseTree::Glob(_) => {}
        }
    }
}

fn is_test_module(item: &Item) -> bool {
    let Item::Mod(module) = item else {
        return false;
    };
    module.attrs.iter().any(|attribute| {
        matches!(&attribute.meta, Meta::List(list)
            if list.path.is_ident("cfg") && list.tokens.to_string() == "test")
    })
}

fn is_bang(trees: &[TokenTree], index: usize) -> bool {
    let is = |offset: usize, c: char| matches!(trees.get(index + offset), Some(TokenTree::Punct(p)) if p.as_char() == c);
    is(0, '!') && !is(1, '=')
}

fn is_path_separator(trees: &[TokenTree], index: usize) -> bool {
    let colon = |i: usize| matches!(trees.get(i), Some(TokenTree::Punct(p)) if p.as_char() == ':');
    colon(index) && colon(index + 1)
}

/// Whether the identifier at `index` is a path's first segment, not a field, method or later segment.
fn starts_a_path(trees: &[TokenTree], index: usize) -> bool {
    let at = |offset: usize| index.checked_sub(offset).and_then(|i| trees.get(i));
    let punct = |tree: Option<&TokenTree>, c: char| matches!(tree, Some(TokenTree::Punct(p)) if p.as_char() == c);
    if punct(at(1), '.') {
        return false;
    }
    if !(punct(at(1), ':') && punct(at(2), ':')) {
        return true;
    }
    match at(3) {
        Some(TokenTree::Ident(owner)) => is_keyword(&owner.unraw().to_string()),
        Some(TokenTree::Punct(p)) if p.as_char() == '>' => punct(at(4), '-') || punct(at(4), '='),
        _ => true,
    }
}

/// A keyword that can stand before a path, so a `::` after it is a leading one.
fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "as" | "break"
            | "const"
            | "dyn"
            | "else"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "match"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "static"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "yield"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forbidden() -> BTreeMap<String, Forbidden> {
        [
            ("store", "explicitly forbidden"),
            ("server", "explicitly forbidden"),
        ]
        .into_iter()
        .map(|(ident, ring)| {
            (
                ident.to_owned(),
                Forbidden {
                    ident: ident.to_owned(),
                    ring: ring.to_owned(),
                },
            )
        })
        .collect()
    }

    fn scan(source: &str) -> Vec<(FileRule, String)> {
        let parsed = syn::parse_file(source).unwrap();
        scan_view(&parsed, Path::new("app/pages/x.rs"), &forbidden())
            .into_iter()
            .map(|(rule, finding)| (rule, finding.to_string()))
            .collect()
    }

    fn rules(source: &str) -> Vec<FileRule> {
        scan(source).into_iter().map(|(rule, _)| rule).collect()
    }

    #[test]
    fn a_humble_view_has_no_finding() {
        let source = "use ui::Card;\nuse domain::Label;\npub fn page(model: Model) -> impl IntoView {\n    view! { <Card title=Label::Title>{model.name}</Card> }\n}\n";
        assert!(scan(source).is_empty(), "{:?}", scan(source));
    }

    #[test]
    fn cfg_is_seen_as_an_attribute_inside_a_macro_and_as_a_macro() {
        let source = "#![cfg_attr(x, y)]\n#[cfg(feature = \"ssr\")]\nuse a::b;\npub fn page() { view! { #[cfg(x)] <p/> }; if cfg!(x) {} let cfg = 1; let _ = cfg != 2; }\n";
        let found = scan(source);
        assert_eq!(rules(source), [FileRule::Cfg; 4], "{found:?}");
        assert!(
            found
                .iter()
                .any(|(_, f)| f.contains("app/pages/x.rs:2") && f.contains("#[cfg]"))
        );
        assert!(found.iter().any(|(_, f)| f.contains("#[cfg_attr]")));
        assert!(found.iter().any(|(_, f)| f.contains("cfg!")));
    }

    #[test]
    fn async_and_await_are_seen_anywhere() {
        let source = "async fn load() {}\npub fn page() { view! { <p on:click=move |_| spawn(async move { go().await })/> } }\n";
        let found = scan(source);
        assert_eq!(
            rules(source),
            [FileRule::Async, FileRule::Async, FileRule::Async],
            "{found:?}"
        );
        assert!(
            found
                .iter()
                .any(|(_, f)| f.contains(":1:") && f.contains("`async`"))
        );
        assert!(
            found
                .iter()
                .any(|(_, f)| f.contains(":2:") && f.contains("`.await`"))
        );
    }

    #[test]
    fn an_adapter_is_seen_however_it_is_named() {
        let source = "use store::Repo;\nuse store as s;\nuse {server, ui};\nextern crate store;\nfn a() { ::store::read(); }\nfn b() { view! { {store::count()} } }\n";
        let found = scan(source);
        assert_eq!(rules(source), [FileRule::Adapter; 6], "{found:?}");
        let leading = "use ::store::Repo;\npub use ::store::Repo;\nfn c() -> ::store::Repo { return ::store::read(); }\nimpl ::store::Trait for X {}\nfn d(r: &mut ::store::Repo, t: &dyn ::store::T) { match ::store::x() { _ => ::store::y() } }\nfn e() { r#store::read(); }\nuse r#store as raw;\n";
        assert_eq!(
            rules(leading),
            [FileRule::Adapter; 11],
            "{:?}",
            scan(leading)
        );
        assert!(
            found
                .iter()
                .any(|(_, f)| f.contains("x.rs:3") && f.contains("forbidden dependency `server`"))
        );
        assert!(
            found
                .iter()
                .all(|(_, f)| f.contains("view names forbidden dependency"))
        );
    }

    #[test]
    fn a_later_segment_or_a_field_of_the_same_name_is_not_a_crate() {
        let source = "fn a() { domain::store::x(); Self::store::y(); <T as Tr>::store::z(); s.store::<u8>(); let store = 1; store; }\n";
        assert!(scan(source).is_empty(), "{:?}", scan(source));
    }

    #[test]
    fn source_the_check_cannot_read_is_refused() {
        let source = "mod other;\n#[path = \"x.rs\"]\nmod inline {}\nconst S: &str = include!(\"y.rs\");\nmacro_rules! via { ($k:ident) => { $k::open() } }\nfn f() { via!(store); }\n";
        let found = scan(source);
        assert_eq!(rules(source), [FileRule::HiddenSource; 4], "{found:?}");
        assert!(found.iter().any(|(_, f)| f.contains("`macro_rules!`")));
    }

    #[test]
    fn a_cfg_test_module_is_allowed() {
        let source = "pub fn page() {}\n\n#[cfg(test)]\nmod tests {\n    use store::Repo;\n    #[tokio::test]\n    async fn renders() { cfg!(x); }\n}\n";
        assert!(scan(source).is_empty(), "{:?}", scan(source));
        let not_only_test = "#[cfg(any(test, feature = \"x\"))]\nmod helpers {}\n";
        assert_eq!(rules(not_only_test), [FileRule::Cfg]);
    }
}
