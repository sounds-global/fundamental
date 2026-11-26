//! Struct-level `#[model(...)]` attributes.

use fundamental_core::model::sql::{Lint, ShardKind};
use syn::{Attribute, Expr, Lit};

/// The backends a model is bound to: at most one of `sql` and `cql`, plus optionally `offline`.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Backends {
    pub sql: bool,
    pub cql: bool,
    pub offline: bool,
}

/// One secondary index declared with `#[model(index(...))]`.
#[derive(Debug, Clone)]
pub(super) struct IndexAttr {
    pub name: syn::LitStr,
    /// Each column with its split and nulls order when written; the literal is kept for errors.
    pub columns: Vec<(syn::LitStr, String, Option<ShardKind>, Option<bool>)>,
    pub unique: bool,
    /// Made by a table's `CONSTRAINT … UNIQUE` in a hand-written migration.
    pub constraint: bool,
    /// The condition of a partial index.
    pub partial: Option<syn::LitStr>,
    /// Columns carried beside the key, so a read needs no row.
    pub include: Vec<syn::LitStr>,
}

/// Struct-level attributes parsed from `#[model(...)]`.
#[derive(Debug, Default)]
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
pub(super) struct StructAttrs {
    pub backends: Backends,
    /// The routing key of a named connection; exclusive with `connection`.
    pub domain: Option<String>,
    /// A typed `Connection` marker whose `NAME` is the routing key; exclusive with `domain`.
    pub connection: Option<syn::Path>,
    pub table: Option<String>,
    pub offline_table: Option<String>,
    /// Keep offline data across schema changes instead of dropping the table.
    pub offline_preserve: bool,
    /// The framework manages a `deleted_at` column that default reads exclude.
    pub soft_delete: bool,
    pub no_from_row: bool,
    /// Source-locale singular label; translation key `model.{type}`.
    pub label: Option<String>,
    /// Source-locale plural label; translation key `model.{type}.plural`.
    pub label_plural: Option<String>,
    pub syncable: bool,
    /// A syncable model that accepts client pushes; `syncable` alone is pull-only.
    pub bidirectional: bool,
    /// Every row carries an `outbox_seal` written on each whole-row write and checked on read.
    pub outbox: bool,
    pub indexes: Vec<IndexAttr>,
    /// Each check's name and SQL condition.
    pub checks: Vec<(syn::LitStr, syn::LitStr)>,
    /// Each reading role with the rows it sees, as an SQL condition.
    pub readers: Vec<(syn::LitStr, syn::LitStr)>,
    pub retired: RetiredAttr,
    pub writers: Vec<syn::LitStr>,
    pub colocated: Option<syn::LitBool>,
    /// The tablets the table is split into when created.
    pub tablets: Option<syn::LitInt>,
    /// Each accepted lint with its reason.
    pub accepted: Vec<(Lint, syn::LitStr)>,
}

/// `retired(columns = [...], indexes = [...], checks = [...])`: names the next migration drops.
#[derive(Debug, Default)]
pub struct RetiredAttr {
    /// Columns to drop.
    pub columns: Vec<syn::LitStr>,
    /// Indexes to drop.
    pub indexes: Vec<syn::LitStr>,
    /// Checks to drop.
    pub checks: Vec<syn::LitStr>,
}

impl StructAttrs {
    #[expect(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        reason = "readability baseline"
    )]
    pub fn from_attrs(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut result = Self::default();

        for attr in attrs {
            if !attr.path().is_ident("model") {
                continue;
            }

            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("sql") {
                    result.backends.sql = true;
                } else if meta.path.is_ident("cql") {
                    result.backends.cql = true;
                } else if meta.path.is_ident("offline") {
                    result.backends.offline = true;
                } else if meta.path.is_ident("domain") {
                    result.domain = Some(parse_str(&meta)?);
                } else if meta.path.is_ident("connection") {
                    result.connection = Some(meta.value()?.parse::<syn::Path>()?);
                } else if meta.path.is_ident("table") {
                    result.table = Some(parse_str(&meta)?);
                } else if meta.path.is_ident("offline_table") {
                    result.offline_table = Some(parse_str(&meta)?);
                } else if meta.path.is_ident("offline_preserve") {
                    result.offline_preserve = true;
                } else if meta.path.is_ident("soft_delete") {
                    result.soft_delete = true;
                } else if meta.path.is_ident("no_from_row") {
                    result.no_from_row = true;
                } else if meta.path.is_ident("syncable") {
                    result.syncable = true;
                } else if meta.path.is_ident("bidirectional") {
                    result.bidirectional = true;
                } else if meta.path.is_ident("outbox") {
                    result.outbox = true;
                } else if meta.path.is_ident("index") {
                    let mut name = None;
                    let mut columns = None;
                    let mut unique = false;
                    let mut constraint = false;
                    let mut partial = None;
                    let mut include = Vec::new();
                    meta.parse_nested_meta(|inner| {
                        if inner.path.is_ident("name") {
                            name = Some(inner.value()?.parse::<syn::LitStr>()?);
                        } else if inner.path.is_ident("columns") {
                            let mut parsed = Vec::new();
                            for column in string_array(&inner)? {
                                parsed.push(index_column(column)?);
                            }
                            columns = Some(parsed);
                        } else if inner.path.is_ident("unique") {
                            unique = true;
                        } else if inner.path.is_ident("constraint") {
                            constraint = true;
                        } else if inner.path.is_ident("partial") {
                            partial = Some(inner.value()?.parse::<syn::LitStr>()?);
                        } else if inner.path.is_ident("include") {
                            include = string_array(&inner)?;
                        } else {
                            return Err(inner.error(
                                "`index(...)` takes `name = \"...\"`, `columns = [\"col [hash|asc|desc]\", ...]`, \
                                 `unique`, `constraint`, `partial = \"condition\"` and `include = [\"...\", ...]`",
                            ));
                        }
                        Ok(())
                    })?;
                    let name = name.ok_or_else(|| meta.error("`index(...)` needs `name = \"...\"`"))?;
                    if constraint && !unique {
                        return Err(meta.error(
                            "`constraint` says a `CONSTRAINT … UNIQUE` made the index; it is `unique` too",
                        ));
                    }
                    let columns = columns
                        .ok_or_else(|| meta.error("`index(...)` needs `columns = [\"...\", ...]`"))?;
                    result.indexes.push(IndexAttr {
                        name,
                        columns,
                        unique,
                        constraint,
                        partial,
                        include,
                    });
                } else if meta.path.is_ident("check") {
                    let mut name = None;
                    let mut expr = None;
                    meta.parse_nested_meta(|inner| {
                        if inner.path.is_ident("name") {
                            name = Some(inner.value()?.parse::<syn::LitStr>()?);
                        } else if inner.path.is_ident("expr") {
                            expr = Some(inner.value()?.parse::<syn::LitStr>()?);
                        } else {
                            return Err(inner.error(
                                "`check(...)` takes `name = \"...\"` and `expr = \"condition\"`",
                            ));
                        }
                        Ok(())
                    })?;
                    result.checks.push((
                        name.ok_or_else(|| meta.error("`check(...)` needs `name = \"...\"`"))?,
                        expr.ok_or_else(|| meta.error("`check(...)` needs `expr = \"...\"`"))?,
                    ));
                } else if meta.path.is_ident("read_by") {
                    let mut role = None;
                    let mut when = None;
                    meta.parse_nested_meta(|inner| {
                        if inner.path.is_ident("role") {
                            role = Some(inner.value()?.parse::<syn::LitStr>()?);
                        } else if inner.path.is_ident("when") {
                            when = Some(inner.value()?.parse::<syn::LitStr>()?);
                        } else {
                            return Err(inner.error(
                                "`read_by(...)` takes `role = \"...\"` and `when = \"condition\"`",
                            ));
                        }
                        Ok(())
                    })?;
                    let role =
                        role.ok_or_else(|| meta.error("`read_by(...)` needs `role = \"...\"`"))?;
                    let spelled = role.value();
                    role_name(&role, "a reading role")?;
                    if result.readers.iter().any(|(other, _)| other.value() == spelled) {
                        return Err(syn::Error::new_spanned(
                            &role,
                            "this role already reads the table; say the rows it sees once",
                        ));
                    }
                    result.readers.push((
                        role,
                        when.ok_or_else(|| meta.error("`read_by(...)` needs `when = \"...\"`"))?,
                    ));
                } else if meta.path.is_ident("written_by") {
                    let mut role = None;
                    meta.parse_nested_meta(|inner| {
                        if inner.path.is_ident("role") {
                            role = Some(inner.value()?.parse::<syn::LitStr>()?);
                            Ok(())
                        } else {
                            Err(inner.error("`written_by(...)` takes `role = \"...\"`"))
                        }
                    })?;
                    let role =
                        role.ok_or_else(|| meta.error("`written_by(...)` needs `role = \"...\"`"))?;
                    role_name(&role, "a writing role")?;
                    if result.writers.iter().any(|other| other.value() == role.value()) {
                        return Err(syn::Error::new_spanned(
                            &role,
                            "this role already writes the table; say it once",
                        ));
                    }
                    result.writers.push(role);
                } else if meta.path.is_ident("colocated") {
                    result.colocated = Some(meta.value()?.parse::<syn::LitBool>()?);
                } else if meta.path.is_ident("tablets") {
                    let tablets = meta.value()?.parse::<syn::LitInt>()?;
                    if tablets.base10_parse::<u32>().map_or(true, |n| n == 0) {
                        return Err(syn::Error::new_spanned(
                            &tablets,
                            "a table is split into at least one tablet",
                        ));
                    }
                    result.tablets = Some(tablets);
                } else if meta.path.is_ident("accept") {
                    meta.parse_nested_meta(|inner| {
                        let named = inner
                            .path
                            .get_ident()
                            .map(ToString::to_string)
                            .unwrap_or_default();
                        let lint = Lint::parse(&named).ok_or_else(|| {
                            let names: Vec<&str> = Lint::ALL.iter().map(|lint| lint.name()).collect();
                            inner.error(format!(
                                "`{named}` is no migration lint; the lints are {}",
                                names.join(", ")
                            ))
                        })?;
                        let reason = inner.value()?.parse::<syn::LitStr>()?;
                        if is_placeholder(&reason.value()) {
                            return Err(syn::Error::new_spanned(
                                &reason,
                                "an acceptance says why the finding is right here; write the reason",
                            ));
                        }
                        if result.accepted.iter().any(|(other, _)| *other == lint) {
                            return Err(inner.error("this lint is already accepted; say why once"));
                        }
                        result.accepted.push((lint, reason));
                        Ok(())
                    })?;
                } else if meta.path.is_ident("retired") {
                    meta.parse_nested_meta(|inner| {
                        let names = string_array(&inner)?;
                        if inner.path.is_ident("columns") {
                            result.retired.columns.extend(names);
                        } else if inner.path.is_ident("indexes") {
                            result.retired.indexes.extend(names);
                        } else if inner.path.is_ident("checks") {
                            result.retired.checks.extend(names);
                        } else {
                            return Err(inner.error(
                                "`retired(...)` takes `columns = [...]`, `indexes = [...]` and \
                                 `checks = [...]`",
                            ));
                        }
                        Ok(())
                    })?;
                } else if meta.path.is_ident("label") {
                    result.label = Some(parse_str(&meta)?);
                } else if meta.path.is_ident("label_plural") {
                    result.label_plural = Some(parse_str(&meta)?);
                } else {
                    let key = meta
                        .path
                        .get_ident()
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "?".to_string());
                    return Err(meta.error(format!(
                        "unknown `#[model(...)]` attribute `{key}`.\n\
                         Valid struct-level attributes:\n  \
                         - sql / cql / offline - target backend(s)\n  \
                         - table = \"name\" - override the table name\n  \
                         - domain = \"name\" - named connection / routing key (the keyspace/schema \
                         comes from the connection)\n  \
                         - connection = <Type> - typed connection marker (a `#[derive(Connection)]` \
                         type); mutually exclusive with `domain`\n  \
                         - index(name = \"n\", columns = [\"a\", ...]) - a secondary index a query may pin (sql)\n  \
                         - check(name = \"n\", expr = \"condition\") - a check every row passes (sql)\n  \
                         - read_by(role = \"r\", when = \"condition\") - a role that reads only those rows (sql)\n  \
                         - written_by(role = \"r\") - a role that reads and writes every row (sql)\n  \
                         - colocated = false - the table gets its own tablets in a colocated database (sql)\n  \
                         - tablets = N - split the table into N tablets when it is created (sql)\n  \
                         - accept(<lint> = \"reason\") - a migration lint that is right here, and why (sql)\n  \
                         - retired(columns = [...], indexes = [...], checks = [...]) - what the next \
                         migration drops (sql)\n  \
                         - offline_table = \"name\" - override the SQLite table name\n  \
                         - offline_preserve - keep offline data across schema changes\n  \
                         - soft_delete - manage a `deleted_at` column; exclude soft-deleted rows from reads\n  \
                         - syncable - participate in offline sync (requires offline; derive a mirror table)\n  \
                         - bidirectional - a syncable table accepts client pushes (default: pull-only)\n  \
                         - outbox - a transactional outbox whose every row is sealed (sql only)\n  \
                         - no_from_row - skip the generated FromRow/ToRow impls"
                    )));
                }
                Ok(())
            })?;
        }

        if result.connection.is_some() && result.domain.is_some() {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "`#[model(connection = ...)]` and `#[model(domain = \"...\")]` are mutually \
                 exclusive — pick one (prefer the typed `connection`).",
            ));
        }

        if result.backends.sql && result.backends.cql {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "a model cannot target both `sql` and `cql` — pick one online backend \
                 (it may still be combined with `offline`).",
            ));
        }

        if result.soft_delete
            && !(result.backends.sql || result.backends.cql || result.backends.offline)
        {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "`#[model(soft_delete)]` requires at least one backend \
                 (`sql`, `cql`, or `offline`).",
            ));
        }

        if result.syncable && !result.backends.offline {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "`#[model(syncable)]` requires the model to be `#[model(offline)]` \
                 (sync keeps a mirror in the local offline database).",
            ));
        }

        if result.bidirectional && !result.syncable {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "`#[model(bidirectional)]` requires `#[model(syncable)]` \
                 (it opts a syncable table into accepting client pushes).",
            ));
        }

        if result.syncable && result.soft_delete {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "`#[model(syncable)]` and `#[model(soft_delete)]` are mutually exclusive: \
                 a syncable model propagates deletes through its `_tombstone` overlay, \
                 which already provides soft-delete semantics on the server table.",
            ));
        }

        if result.outbox
            && (!result.backends.sql
                || result.backends.offline
                || result.syncable
                || result.no_from_row)
        {
            return Err(syn::Error::new_spanned(
                &attrs[0],
                "`#[model(outbox)]` requires `sql` and cannot be combined with `offline`, \
                 `syncable` or `no_from_row`: the seal is written and checked by the generated \
                 SQL row codec, on the table a relay claims from.",
            ));
        }

        result.placement()?;
        Ok(result)
    }

    /// Refuses a split table that is colocated, and a role that both reads by `read_by` and writes.
    fn placement(&self) -> syn::Result<()> {
        if let (Some(colocated), Some(tablets)) = (&self.colocated, &self.tablets)
            && colocated.value
        {
            return Err(syn::Error::new_spanned(
                tablets,
                "a colocated table shares one tablet; `tablets` splits a table out of colocation, \
                 so leave `colocated` off or say `colocated = false`",
            ));
        }
        for writer in &self.writers {
            if self
                .readers
                .iter()
                .any(|(reader, _)| reader.value() == writer.value())
            {
                return Err(syn::Error::new_spanned(
                    writer,
                    "a writing role reads every row, and `read_by` narrows the rows this role \
                     sees; give it one or the other",
                ));
            }
        }
        Ok(())
    }
}

/// Refuses a role that is not a lowercase identifier, `what` naming the role's kind.
fn role_name(role: &syn::LitStr, what: &str) -> syn::Result<()> {
    let spelled = role.value();
    let mut chars = spelled.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_lowercase())
        || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(syn::Error::new_spanned(
            role,
            format!("{what} is a lowercase identifier"),
        ));
    }
    Ok(())
}

/// Whether `reason` says nothing: empty, or a word that stands in for a reason.
fn is_placeholder(reason: &str) -> bool {
    let said = reason
        .trim()
        .trim_matches(|c: char| c == '.' || c == '!' || c == '?')
        .to_ascii_lowercase();
    said.chars().filter(|c| c.is_alphanumeric()).count() < 3
        || matches!(
            said.as_str(),
            "why"
                | "todo"
                | "tbd"
                | "fixme"
                | "reason"
                | "because"
                | "n/a"
                | "none"
                | "ok"
                | "fine"
        )
}

/// Parses a `key = "value"` string literal from a nested meta entry.
fn parse_str(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<String> {
    let value: Expr = meta.value()?.parse()?;
    if let Expr::Lit(expr_lit) = value
        && let Lit::Str(lit_str) = expr_lit.lit
    {
        return Ok(lit_str.value());
    }
    Err(meta.error("expected a string literal"))
}

/// The string literals of `[ "...", ... ]`, the value of `inner`.
fn string_array(inner: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<Vec<syn::LitStr>> {
    let array = inner.value()?.parse::<syn::ExprArray>()?;
    array
        .elems
        .into_iter()
        .map(|element| match element {
            Expr::Lit(syn::ExprLit {
                lit: Lit::Str(column),
                ..
            }) => Ok(column),
            other => Err(syn::Error::new_spanned(
                other,
                "an index column is a string literal naming a column",
            )),
        })
        .collect()
}

/// An index column as written (`"at desc nulls last"`): its name, split and nulls order.
fn index_column(
    column: syn::LitStr,
) -> syn::Result<(syn::LitStr, String, Option<ShardKind>, Option<bool>)> {
    let written = column.value();
    let words: Vec<&str> = written.split_whitespace().collect();
    let refused = || {
        syn::Error::new_spanned(
            &column,
            "an index column is its name, optionally how it is split and where its empty values go: \
             \"slug asc\", \"at desc nulls last\"",
        )
    };
    let (name, rest) = words.split_first().ok_or_else(refused)?;
    let (split, rest) = match rest.split_first() {
        Some((word, rest)) if !word.eq_ignore_ascii_case("nulls") => {
            (Some(ShardKind::parse_str(word).ok_or_else(refused)?), rest)
        }
        _ => (None, rest),
    };
    let nulls_first = match rest {
        [] => None,
        [nulls, place] if nulls.eq_ignore_ascii_case("nulls") => {
            match place.to_ascii_lowercase().as_str() {
                "first" => Some(true),
                "last" => Some(false),
                _ => return Err(refused()),
            }
        }
        _ => return Err(refused()),
    };
    Ok((column.clone(), (*name).to_owned(), split, nulls_first))
}

#[cfg(test)]
mod tests {
    use fundamental_core::model::sql::Lint;

    use super::StructAttrs;

    fn parsed(attr: syn::Attribute) -> syn::Result<StructAttrs> {
        StructAttrs::from_attrs(&[attr])
    }

    fn refused(attr: syn::Attribute) -> String {
        parsed(attr).expect_err("refused").to_string()
    }

    #[test]
    fn placement_writers_and_acceptances_are_read() {
        let attrs = parsed(syn::parse_quote!(#[model(
            sql,
            colocated = false,
            tablets = 8,
            written_by(role = "importer"),
            accept(hot_shard = "read newest first by one importer")
        )]))
        .expect("read");

        assert!(attrs.colocated.is_some_and(|colocated| !colocated.value));
        assert_eq!(
            attrs.tablets.map(|n| n.base10_parse::<u32>().ok()),
            Some(Some(8))
        );
        assert_eq!(attrs.writers[0].value(), "importer");
        assert_eq!(attrs.accepted[0].0, Lint::HotShard);
    }

    #[test]
    fn a_lint_there_is_none_of_is_refused_naming_the_lints() {
        let error = refused(syn::parse_quote!(#[model(sql, accept(hot_shards = "a reason"))]));

        assert!(
            error.contains("`hot_shards` is no migration lint"),
            "{error}"
        );
        assert!(error.contains("hot_shard"), "{error}");
    }

    #[test]
    fn an_acceptance_without_a_reason_is_refused() {
        for reason in ["", "why", "TODO", "ok."] {
            let attr: syn::Attribute = syn::parse_quote!(#[model(sql, accept(backfill = #reason))]);
            assert!(refused(attr).contains("write the reason"), "{reason}");
        }
    }

    #[test]
    fn a_colocated_table_split_into_tablets_is_refused() {
        let error = refused(syn::parse_quote!(#[model(sql, colocated = true, tablets = 4)]));

        assert!(error.contains("shares one tablet"), "{error}");
    }

    #[test]
    fn no_tablets_is_refused() {
        assert!(refused(syn::parse_quote!(#[model(sql, tablets = 0)])).contains("at least one"));
    }

    #[test]
    fn a_writer_is_a_lowercase_identifier_said_once_and_no_reader() {
        assert!(
            refused(syn::parse_quote!(#[model(sql, written_by(role = "Importer"))]))
                .contains("a writing role is a lowercase identifier")
        );
        assert!(
            refused(syn::parse_quote!(#[model(
                sql,
                written_by(role = "importer"),
                written_by(role = "importer")
            )]))
            .contains("already writes")
        );
        assert!(
            refused(syn::parse_quote!(#[model(
                sql,
                read_by(role = "www", when = "true"),
                written_by(role = "www")
            )]))
            .contains("one or the other")
        );
    }
}
