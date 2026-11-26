//! Field-level `#[model(...)]` attributes.

use syn::{Attribute, Expr, Lit};

use fundamental_core::model::cql::ClusteringOrder;
use fundamental_core::model::sql::ShardKind;

/// Field-level attributes parsed from `#[model(...)]`.
#[derive(Debug, Default, Clone)]
#[expect(clippy::struct_excessive_bools, reason = "readability baseline")]
pub(super) struct FieldAttrs {
    pub is_id: bool,
    /// Only read where the model's own DDL is emitted; a migration declares its own sharding.
    pub shard: Option<ShardKind>,
    pub partition_key: bool,
    pub clustering_key: Option<ClusteringOrder>,
    pub indexed: bool,
    pub encrypted: bool,
    pub encryption_key: Option<String>,
    pub column: Option<String>,
    pub skip: bool,
    pub is_enum: bool,
    pub is_variant: bool,
    /// Source-locale label; translation key `model.{type}.field.{field}`.
    pub label: Option<String>,
    /// The column's default, as SQL.
    pub default: Option<String>,
    /// The column's previous name, so the next migration renames it.
    pub was: Option<String>,
    /// Excluded from offline sync; secret fields are excluded regardless.
    pub no_sync: bool,
    /// The slice's per-principal scoping column; `CqlSliceStore` partitions its outbox by it.
    pub owner: bool,
    /// Left out of the row binding encrypted fields are sealed to.
    pub unbound: bool,
    /// An erasure may clear it, so it is left out of the outbox seal.
    pub erasable: bool,
}

impl FieldAttrs {
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
                if meta.path.is_ident("id") {
                    result.is_id = true;
                } else if meta.path.is_ident("shard") {
                    let value: Expr = meta.value()?.parse()?;
                    let raw = match &value {
                        // Both `shard = hash` and `shard = "hash"` are accepted.
                        Expr::Path(path) => path.path.get_ident().map(ToString::to_string),
                        Expr::Lit(lit) => match &lit.lit {
                            Lit::Str(s) => Some(s.value()),
                            _ => None,
                        },
                        _ => None,
                    };
                    let parsed = raw.as_deref().and_then(ShardKind::parse_str);
                    result.shard = Some(parsed.ok_or_else(|| {
                        meta.error(
                            "`shard` takes hash, asc or desc: HASH for a point lookup, \
                             ASC or DESC where the access pattern is an ordered or \
                             ranged scan",
                        )
                    })?);
                } else if meta.path.is_ident("partition_key") {
                    result.partition_key = true;
                } else if meta.path.is_ident("clustering_key") {
                    if meta.input.peek(syn::Token![=]) {
                        let value: Expr = meta.value()?.parse()?;
                        if let Expr::Lit(expr_lit) = value
                            && let Lit::Str(lit_str) = expr_lit.lit
                        {
                            result.clustering_key = ClusteringOrder::parse_str(&lit_str.value())
                                .or(Some(ClusteringOrder::Asc));
                        }
                    } else {
                        result.clustering_key = Some(ClusteringOrder::Asc);
                    }
                } else if meta.path.is_ident("indexed") {
                    result.indexed = true;
                } else if meta.path.is_ident("encrypted") {
                    result.encrypted = true;
                    if meta.input.peek(syn::Token![=]) {
                        let value: Expr = meta.value()?.parse()?;
                        if let Expr::Lit(expr_lit) = value
                            && let Lit::Str(lit_str) = expr_lit.lit
                        {
                            result.encryption_key = Some(lit_str.value());
                        }
                    }
                } else if meta.path.is_ident("skip") {
                    result.skip = true;
                } else if meta.path.is_ident("no_sync") {
                    result.no_sync = true;
                } else if meta.path.is_ident("owner") {
                    result.owner = true;
                } else if meta.path.is_ident("unbound") {
                    result.unbound = true;
                } else if meta.path.is_ident("erasable") {
                    result.erasable = true;
                } else if meta.path.is_ident("r#enum") || meta.path.is_ident("enum") {
                    result.is_enum = true;
                } else if meta.path.is_ident("variant") {
                    result.is_variant = true;
                } else if meta.path.is_ident("column") {
                    let value: Expr = meta.value()?.parse()?;
                    if let Expr::Lit(expr_lit) = value
                        && let Lit::Str(lit_str) = expr_lit.lit
                    {
                        result.column = Some(lit_str.value());
                    }
                } else if meta.path.is_ident("default") {
                    result.default = Some(meta.value()?.parse::<syn::LitStr>()?.value());
                } else if meta.path.is_ident("was") {
                    result.was = Some(meta.value()?.parse::<syn::LitStr>()?.value());
                } else if meta.path.is_ident("label") {
                    let value: Expr = meta.value()?.parse()?;
                    if let Expr::Lit(expr_lit) = value
                        && let Lit::Str(lit_str) = expr_lit.lit
                    {
                        result.label = Some(lit_str.value());
                    }
                } else {
                    let path_str = meta
                        .path
                        .get_ident()
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    return Err(meta.error(format!(
                        "Unknown model field attribute '{}'.\n\
                         Valid attributes:\n  \
                         - id - Primary key\n  \
                         - shard = hash|asc|desc - How a SQL key column distributes\n  \
                         - partition_key - YCQL partition key\n  \
                         - clustering_key - YCQL clustering key\n  \
                         - indexed - Create database index\n  \
                         - encrypted / encrypted = \"key\" - Field-level encryption\n  \
                         - column = \"name\" - Custom column name\n  \
                         - label = \"Name\" - Human-readable field label (i18n)\n  \
                         - skip - Skip in serialization\n  \
                         - no_sync - Exclude from offline sync (syncable models)\n  \
                         - owner - Slice scoping column (syncable models)\n  \
                         - unbound - Bookkeeping column left out of the row binding\n  \
                         - erasable - Column an erasure may clear, left out of an outbox seal\n  \
                         - r#enum - Enum field (stored as string, parsed via FromStr)\n  \
                         - variant - A `Variant`, stored as its value\n  \
                         - default = \"sql\" - The column's default, as SQL\n  \
                         - was = \"name\" - The column's name before, so a migration renames it",
                        path_str
                    )));
                }
                Ok(())
            })?;
        }

        Ok(result)
    }
}
