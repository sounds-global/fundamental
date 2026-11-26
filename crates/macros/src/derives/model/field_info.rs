//! Per-field helpers shared across the `Model` derive's backend arms.

use quote::quote;
use syn::{Ident, Type};

use fundamental_core::model::cql::ClusteringOrder;
use fundamental_core::model::sql::ShardKind;

use super::FieldAttrs;

/// Information about a parsed field.
pub(super) struct FieldInfo<'a> {
    pub ident: &'a Ident,
    pub ty: &'a Type,
    pub attrs: FieldAttrs,
}

/// The backend dialect a [`ColumnSpec`] is computed for.
#[derive(Clone, Copy)]
pub(super) enum SchemaBackend {
    /// YSQL.
    Postgres,
    /// Offline SQLite.
    Sqlite,
    /// YCQL.
    Cql,
}

/// One column as both the DDL and the `ModelSchema` builder see it, so the two never disagree.
pub(super) struct ColumnSpec {
    /// Column name (honoring `#[model(column = "…")]`).
    pub name: String,
    /// Backend storage type (e.g. `TEXT`, `INT8`, `bigint`).
    pub storage_type: String,
    /// Where the macro cannot name the type, the expression reading it from the type's `SqlType`.
    pub storage_type_expr: Option<proc_macro2::TokenStream>,
    /// The column's default, as SQL, when the model declares one.
    pub default: Option<String>,
    pub nullable: bool,
    pub primary_key: bool,
    /// How the key column distributes, when `#[model(shard = …)]` said.
    pub shard: Option<ShardKind>,
}

impl<'a> FieldInfo<'a> {
    /// The final path-segment ident of a type, so `InMemorySecret` never passes for `Secret`.
    fn last_type_ident(ty: &Type) -> Option<String> {
        match ty {
            Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.to_string()),
            _ => None,
        }
    }

    /// Whether a type is a persisted `Secret`-family wrapper, encrypted at rest.
    pub fn is_secret_type(ty: &Type) -> bool {
        matches!(
            Self::last_type_ident(ty).as_deref(),
            Some("Secret" | "SecretString" | "SecretBytes" | "SecretJson" | "DatabaseUrl")
        )
    }

    /// The `T` of a leading `Option<T>`.
    fn option_inner(ty: &Type) -> Option<&Type> {
        let Type::Path(tp) = ty else { return None };
        let seg = tp.path.segments.last()?;
        if seg.ident != "Option" {
            return None;
        }
        match &seg.arguments {
            syn::PathArguments::AngleBracketed(args) => match args.args.first() {
                Some(syn::GenericArgument::Type(inner)) => Some(inner),
                _ => None,
            },
            _ => None,
        }
    }

    /// The inner secret type of an `Option<Secret*>` field.
    pub fn optional_secret_inner(ty: &Type) -> Option<&Type> {
        Self::option_inner(ty).filter(|inner| Self::is_secret_type(inner))
    }

    /// Whether this field is encrypted: a secret or an `Option` of one.
    pub fn is_encrypted_column(ty: &Type) -> bool {
        Self::is_secret_type(ty) || Self::optional_secret_inner(ty).is_some()
    }

    /// The secret type to dispatch on, looking through an `Option`.
    fn secret_carrier(ty: &Type) -> &Type {
        Self::optional_secret_inner(ty).unwrap_or(ty)
    }

    /// Whether a type is `SecretBytes`, looking through an `Option`.
    pub fn is_secret_bytes_type(ty: &Type) -> bool {
        Self::last_type_ident(Self::secret_carrier(ty)).as_deref() == Some("SecretBytes")
    }

    /// Whether a type is `SecretJson<T>`, looking through an `Option`.
    pub fn is_secret_json_type(ty: &Type) -> bool {
        Self::last_type_ident(Self::secret_carrier(ty)).as_deref() == Some("SecretJson")
    }

    /// The `T` of a `SecretJson<T>` field, looking through an `Option`.
    pub fn secret_json_inner(ty: &Type) -> Option<&Type> {
        let carrier = Self::secret_carrier(ty);
        let Type::Path(tp) = carrier else {
            return None;
        };
        let seg = tp.path.segments.last()?;
        if seg.ident != "SecretJson" {
            return None;
        }
        match &seg.arguments {
            syn::PathArguments::AngleBracketed(args) => match args.args.first() {
                Some(syn::GenericArgument::Type(inner)) => Some(inner),
                _ => None,
            },
            _ => None,
        }
    }

    /// Whether a type is an `InMemorySecret` wrapper, which the derive rejects as a field.
    pub fn is_in_memory_secret_type(ty: &Type) -> bool {
        matches!(
            Self::last_type_ident(ty).as_deref(),
            Some("InMemorySecret" | "InMemorySecretString")
        )
    }

    /// The field's encryption key id, `"default"` when none is given.
    pub fn encryption_key_id(&self) -> String {
        self.attrs
            .encryption_key
            .clone()
            .unwrap_or_else(|| "default".to_string())
    }

    pub fn is_option_type(ty: &Type) -> bool {
        let type_str = quote!(#ty).to_string();
        type_str.starts_with("Option <") || type_str.starts_with("Option<")
    }

    /// Whether this is a moment (`DateTime<…>`, or an `Option` of one), which a `TIMESTAMPTZ`
    /// column keeps to the microsecond.
    pub fn is_moment_type(ty: &Type) -> bool {
        Self::terminal_ident(ty).as_deref() == Some("DateTime")
    }

    pub fn is_localized_type(ty: &Type) -> bool {
        Self::localized_inner(ty).is_some()
    }

    /// The `T` of a `Localized<T>` field, stored as TEXT through `Display`/`FromStr`.
    pub fn localized_inner(ty: &Type) -> Option<&Type> {
        let Type::Path(tp) = ty else { return None };
        let seg = tp.path.segments.last()?;
        if seg.ident != "Localized" {
            return None;
        }
        match &seg.arguments {
            syn::PathArguments::AngleBracketed(args) => match args.args.first() {
                Some(syn::GenericArgument::Type(inner)) => Some(inner),
                _ => None,
            },
            _ => None,
        }
    }

    /// Pluralizes an English word; an irregular noun needs `#[model(table = "…")]`.
    pub fn pluralize(word: &str) -> String {
        if word.is_empty() {
            return word.to_string();
        }

        if word.ends_with('s') && !word.ends_with("ss") {
            return word.to_string();
        }

        if let Some(plural) = Self::f_to_ves(word) {
            return plural.to_string();
        }

        if word.ends_with("ch")
            || word.ends_with("sh")
            || word.ends_with('x')
            || word.ends_with('z')
            || word.ends_with("ss")
        {
            return format!("{}es", word);
        }

        if let Some(stem) = word.strip_suffix('y') {
            let before_y = stem.as_bytes().last().copied().unwrap_or(b'a');
            if !matches!(before_y, b'a' | b'e' | b'i' | b'o' | b'u') {
                return format!("{stem}ies");
            }
        }

        format!("{word}s")
    }

    /// The `-f`/`-fe` words that take `-ves`; the rest (roof, chief) take a plain `-s`.
    fn f_to_ves(word: &str) -> Option<&'static str> {
        Some(match word {
            "leaf" => "leaves",
            "loaf" => "loaves",
            "sheaf" => "sheaves",
            "thief" => "thieves",
            "knife" => "knives",
            "life" => "lives",
            "wife" => "wives",
            "half" => "halves",
            "calf" => "calves",
            "elf" => "elves",
            "self" => "selves",
            "shelf" => "shelves",
            "wolf" => "wolves",
            "hoof" => "hooves",
            "scarf" => "scarves",
            _ => return None,
        })
    }

    /// `OrganizationMember` → `organization_members`: snake_case, the last word pluralized.
    pub fn infer_table_name(struct_name: &str) -> String {
        let snake = Self::to_snake_case(struct_name);
        if let Some(pos) = snake.rfind('_') {
            let prefix = &snake[..pos];
            let last_word = &snake[pos + 1..];
            format!("{}_{}", prefix, Self::pluralize(last_word))
        } else {
            Self::pluralize(&snake)
        }
    }

    #[expect(clippy::excessive_nesting, reason = "readability baseline")]
    pub fn to_snake_case(name: &str) -> String {
        let mut result = String::new();
        for (i, c) in name.chars().enumerate() {
            if c.is_uppercase() {
                if i > 0 {
                    result.push('_');
                }
                result.push(c.to_ascii_lowercase());
            } else {
                result.push(c);
            }
        }
        result
    }

    /// A hash of the table name and each column's name, type, order and nullability.
    pub fn compute_schema_hash(table_name: &str, columns: &[(String, String, bool)]) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();

        table_name.hash(&mut hasher);

        for (name, sql_type, nullable) in columns {
            name.hash(&mut hasher);
            sql_type.hash(&mut hasher);
            nullable.hash(&mut hasher);
        }

        hasher.finish()
    }

    /// Whether the YSQL type is the Rust type's `SqlType`: not an enum, secret or translatable
    /// text.
    fn asks_sql_type(&self) -> bool {
        let known = matches!(
            Self::terminal_ident(self.ty).as_deref(),
            Some("String" | "str")
        );
        !known
            && !self.attrs.is_enum
            && !Self::is_encrypted_column(self.ty)
            && !Self::is_localized_type(self.ty)
    }

    /// This field's [`ColumnSpec`] in `backend`, or `None` when skipped.
    pub fn column_spec(&self, backend: SchemaBackend) -> Option<ColumnSpec> {
        if self.attrs.skip {
            return None;
        }
        let name = self
            .attrs
            .column
            .clone()
            .unwrap_or_else(|| self.ident.to_string());
        let storage_type = match backend {
            SchemaBackend::Postgres => Self::rust_type_to_postgres(self.ty),
            SchemaBackend::Sqlite => Self::rust_type_to_sqlite(self.ty),
            SchemaBackend::Cql => Self::rust_type_to_cql(self.ty),
        }
        .to_string();
        let storage_type_expr = (matches!(backend, SchemaBackend::Postgres)
            && storage_type == "TEXT"
            && self.asks_sql_type())
        .then(|| {
            let core = crate::derives::core_path();
            let ty = self.ty;
            quote! { <#ty as #core::model::sql::SqlType>::SQL }
        });
        Some(ColumnSpec {
            name,
            storage_type,
            storage_type_expr,
            default: self.attrs.default.clone(),
            nullable: Self::is_option_type(self.ty),
            primary_key: self.attrs.is_id,
            shard: self.attrs.shard,
        })
    }
}

impl<'a> FieldInfo<'a> {
    /// The field initializer `field_ident: <value>` for a plain CQL read.
    pub fn cql_from_row_field_extraction(
        field_ident: &Ident,
        field_ty: &Type,
        idx: usize,
        col_name: &str,
    ) -> proc_macro2::TokenStream {
        let value = Self::cql_from_row_value_extraction(field_ty, idx, col_name);
        quote! { #field_ident: #value }
    }

    /// The decoded value of a CQL column at `idx`, as an expression over `cols`.
    pub fn cql_from_row_value_extraction(
        field_ty: &Type,
        idx: usize,
        col_name: &str,
    ) -> proc_macro2::TokenStream {
        let cql = crate::derives::cql_path();
        let ty_str = quote!(#field_ty).to_string().replace(' ', "");

        // The source value is inline TEXT; other locales are hydrated from the companion table.
        if let Some(inner) = Self::localized_inner(field_ty) {
            let core = crate::derives::core_path();
            return quote! {
                {
                    let v = cols.get(#idx)
                        .ok_or_else(|| #cql::CqlError::RowConversion(
                            format!("column {} (index {}) missing from row", #col_name, #idx)))?
                        .as_ref()
                        .ok_or_else(|| #cql::CqlError::RowConversion(
                            format!("column {} (index {}) is null", #col_name, #idx)))?;
                    let __s = v.as_text().ok_or_else(|| #cql::CqlError::RowConversion(
                        format!("column {} (index {}): expected text", #col_name, #idx)))?;
                    #core::i18n::Localized::new(
                        __s.parse::<#inner>().map_err(|_| #cql::CqlError::RowConversion(
                            format!("column {} (index {}): parse error", #col_name, #idx)))?,
                    )
                }
            };
        }

        if ty_str.starts_with("Option<") {
            let inner = &ty_str["Option<".len()..ty_str.len() - 1];
            let extract = Self::cql_value_extract(inner, col_name, idx);
            // A present value that fails to decode is an error, not `None`; the annotation pins the
            // primitive arms' error type.
            return quote! {
                match cols.get(#idx).and_then(|c| c.as_ref()) {
                    ::core::option::Option::Some(v) => {
                        let __decoded: ::core::result::Result<_, #cql::CqlError> = #extract;
                        __decoded?
                    }
                    ::core::option::Option::None => ::core::option::Option::None,
                }
            };
        }

        let extract = Self::cql_value_extract(&ty_str, col_name, idx);
        quote! {
            {
                let v = cols.get(#idx)
                    .ok_or_else(|| #cql::CqlError::RowConversion(
                        format!("column {} (index {}) missing from row", #col_name, #idx)
                    ))?
                    .as_ref()
                    .ok_or_else(|| #cql::CqlError::RowConversion(
                        format!("column {} (index {}) is null", #col_name, #idx)
                    ))?;
                let __decoded: ::core::result::Result<_, #cql::CqlError> = #extract;
                __decoded?.ok_or_else(|| #cql::CqlError::RowConversion(
                    format!("column {} (index {}): type mismatch", #col_name, #idx)
                ))?
            }
        }
    }

    /// A `Result<Option<T>, CqlError>` over `v: &CqlValue`: `Ok(None)` for another CQL type, `Err`
    /// when a value of the right type fails to decode.
    fn cql_value_extract(ty_str: &str, col_name: &str, idx: usize) -> proc_macro2::TokenStream {
        let cql = crate::derives::cql_path();
        match ty_str {
            "String" => quote! { ::core::result::Result::Ok(v.as_text().cloned()) },
            "i64" => quote! { ::core::result::Result::Ok(v.as_bigint()) },
            "i32" => quote! { ::core::result::Result::Ok(v.as_int()) },
            "i16" => quote! { ::core::result::Result::Ok(v.as_smallint()) },
            "i8" => quote! { ::core::result::Result::Ok(v.as_tinyint()) },
            "f64" => quote! { ::core::result::Result::Ok(v.as_double()) },
            "f32" => quote! { ::core::result::Result::Ok(v.as_float()) },
            "bool" => quote! { ::core::result::Result::Ok(v.as_boolean()) },
            "uuid::Uuid" | "Uuid" => quote! { ::core::result::Result::Ok(v.as_uuid()) },
            "Vec<u8>" => quote! { ::core::result::Result::Ok(v.as_blob().cloned()) },
            "DateTime<Utc>" | "chrono::DateTime<chrono::Utc>" => {
                // Out of range is an error, not the Unix epoch.
                quote! {
                    match v.as_cql_timestamp() {
                        ::core::option::Option::Some(__ts) => ::chrono::DateTime::from_timestamp_millis(__ts.0)
                            .map(::core::option::Option::Some)
                            .ok_or_else(|| #cql::CqlError::RowConversion(format!(
                                "column {} (index {}): timestamp {} ms out of range",
                                #col_name, #idx, __ts.0
                            ))),
                        ::core::option::Option::None => ::core::result::Result::Ok(::core::option::Option::None),
                    }
                }
            }
            _ => {
                // `FromCqlValue`, which `#[derive(Identifier)]` implements; text `From<String>`
                // when the type cannot be parsed.
                match syn::parse_str::<syn::Type>(ty_str) {
                    Ok(ty) => {
                        quote! { ::core::result::Result::Ok(<#ty as #cql::FromCqlValue>::from_cql_value(v)) }
                    }
                    Err(_) => {
                        quote! { ::core::result::Result::Ok(v.as_text().map(|s| s.clone().into())) }
                    }
                }
            }
        }
    }

    /// Maps a Rust type to its YCQL storage type; unknown types are `text`.
    pub fn rust_type_to_cql(ty: &Type) -> &'static str {
        if Self::is_byte_vec(ty) {
            return "blob";
        }
        // A secret column is TEXT whatever its plaintext type, `SecretBytes` included.
        if Self::is_encrypted_column(ty) || Self::is_localized_type(ty) {
            return "text";
        }
        match Self::terminal_ident(ty).as_deref() {
            Some("String" | "str") => "text",
            Some("bool") => "boolean",
            Some("i8") => "tinyint",
            Some("i16" | "u8") => "smallint",
            Some("i32" | "u16") => "int",
            Some("i64" | "u32" | "u64" | "usize" | "isize") => "bigint",
            Some("f32") => "float",
            Some("f64") => "double",
            Some("Uuid") => "uuid",
            Some("DateTime") => "timestamp",
            _ => "text",
        }
    }

    /// A YCQL `CREATE TABLE`, the clustering order explicit because CQL silently defaults to `ASC`;
    /// with `soft_delete`, a `deleted_at` column and a secondary index on it.
    #[expect(clippy::too_many_arguments, reason = "readability baseline")]
    pub fn generate_cql_create_table_sql(
        table_name: &str,
        fields: &[FieldInfo<'_>],
        partition_cols: &[String],
        clustering: &[(String, ClusteringOrder)],
        soft_delete: bool,
        syncable: bool,
    ) -> String {
        let mut columns: Vec<String> = fields
            .iter()
            .filter_map(|f| f.column_spec(SchemaBackend::Cql))
            .map(|spec| format!("{} {}", spec.name, spec.storage_type))
            .collect();
        if soft_delete {
            columns.push("deleted_at timestamp".to_string());
        }
        if syncable {
            // The change-feed is a separate outbox table (YCQL cannot range-scan `updated_at`).
            columns.push("updated_at timestamp".to_string());
            columns.push("version bigint".to_string());
            columns.push("_tombstone boolean".to_string());
        }
        let pk = if clustering.is_empty() {
            format!("(({}))", partition_cols.join(", "))
        } else {
            let clustering_cols: Vec<&str> =
                clustering.iter().map(|(name, _)| name.as_str()).collect();
            format!(
                "(({}), {})",
                partition_cols.join(", "),
                clustering_cols.join(", ")
            )
        };
        let mut sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (\n    {},\n    PRIMARY KEY {}\n)",
            table_name,
            columns.join(",\n    "),
            pk
        );
        let mut properties: Vec<String> = Vec::new();
        if !clustering.is_empty() {
            // Explicit so it survives `system_schema` introspection.
            let orders = clustering
                .iter()
                .map(|(name, order)| format!("{} {}", name, order.as_cql()))
                .collect::<Vec<_>>()
                .join(", ");
            properties.push(format!("CLUSTERING ORDER BY ({orders})"));
        }
        if soft_delete {
            // The index needs transactions, and YCQL refuses `USING TTL` on a transactional table,
            // so soft-delete and a per-write TTL exclude each other.
            properties.push("transactions = { 'enabled' : true }".to_string());
        }
        if !properties.is_empty() {
            sql.push_str(&format!(" WITH {}", properties.join(" AND ")));
        }
        if soft_delete {
            sql.push_str(&format!(
                ";\nCREATE INDEX IF NOT EXISTS {table_name}_deleted_at_idx ON {table_name} (deleted_at)"
            ));
        }
        sql
    }
}

impl<'a> FieldInfo<'a> {
    /// The terminal path-segment ident of a type, through one `Option` (`Option<Uuid>` → `Uuid`).
    fn terminal_ident(ty: &Type) -> Option<String> {
        let Type::Path(tp) = ty else { return None };
        let seg = tp.path.segments.last()?;
        if seg.ident == "Option"
            && let syn::PathArguments::AngleBracketed(args) = &seg.arguments
            && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
        {
            return Self::terminal_ident(inner);
        }
        Some(seg.ident.to_string())
    }

    /// Whether the type is `Vec<u8>` (after unwrapping `Option`), which maps to BLOB.
    fn is_byte_vec(ty: &Type) -> bool {
        let inner = match ty {
            Type::Path(tp) if tp.path.segments.last().is_some_and(|s| s.ident == "Option") => {
                match &tp.path.segments.last().unwrap().arguments {
                    syn::PathArguments::AngleBracketed(a) => match a.args.first() {
                        Some(syn::GenericArgument::Type(t)) => t,
                        _ => ty,
                    },
                    _ => ty,
                }
            }
            other => other,
        };
        let Type::Path(tp) = inner else { return false };
        let Some(seg) = tp.path.segments.last() else {
            return false;
        };
        if seg.ident != "Vec" {
            return false;
        }
        matches!(&seg.arguments, syn::PathArguments::AngleBracketed(a)
            if matches!(a.args.first(), Some(syn::GenericArgument::Type(Type::Path(p)))
                if p.path.is_ident("u8")))
    }

    /// Maps a Rust type to its SQLite type by its terminal segment, so `StringId` is not
    /// mis-mapped.
    pub fn rust_type_to_sqlite(ty: &Type) -> &'static str {
        if Self::is_byte_vec(ty) {
            return "BLOB";
        }
        match Self::terminal_ident(ty).as_deref() {
            Some("String" | "str") => "TEXT",
            Some(
                "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "usize" | "isize"
                | "bool",
            ) => "INTEGER",
            Some("f32" | "f64") => "REAL",
            _ => "TEXT",
        }
    }

    /// The `OfflineRecord::to_values` expression for one field, matching its SQLite column type.
    pub fn offline_to_value(&self) -> proc_macro2::TokenStream {
        let offline = crate::derives::offline_path();
        let ident = self.ident;
        let value = quote! { #offline::db::Value };

        if Self::is_secret_type(self.ty) {
            // The device has no data key to encrypt with yet, so this fails loudly instead of
            // storing the redacted form. Behind a `-> String` fn so the value tuple has no
            // `unreachable_code`.
            let field = self.ident.to_string();
            return quote! {
                #value::from({
                    fn __secret_write_unsupported(col: &str) -> ::std::string::String {
                        ::core::panic!(
                            "offline write-back of Secret column `{}` requires a device key (not yet supported)",
                            col
                        )
                    }
                    __secret_write_unsupported(#field)
                })
            };
        }
        if Self::is_localized_type(self.ty) {
            // The source-locale value is inline TEXT; other locales go to the companion table.
            let core = crate::derives::core_path();
            return quote! {
                #value::from(::std::string::ToString::to_string(#core::i18n::Localized::source(&self.#ident)))
            };
        }
        if let Some(text) = self.closed_value_text() {
            return quote! { #value::from(#text) };
        }
        if Self::is_byte_vec(self.ty) {
            return quote! { #value::from(self.#ident.clone()) }; // Vec<u8> / Option<Vec<u8>> → BLOB
        }

        let is_opt = Self::is_option_type(self.ty);
        match Self::terminal_ident(self.ty).as_deref() {
            Some("String" | "str" | "bool" | "i32" | "i64" | "f64") => {
                quote! { #value::from(self.#ident.clone()) }
            }
            Some("i8" | "i16" | "u8" | "u16" | "u32" | "u64" | "usize" | "isize") => {
                if is_opt {
                    quote! { #value::from(self.#ident.map(|v| v as i64)) }
                } else {
                    quote! { #value::from(self.#ident as i64) }
                }
            }
            Some("f32") => {
                if is_opt {
                    quote! { #value::from(self.#ident.map(|v| v as f64)) }
                } else {
                    quote! { #value::from(self.#ident as f64) }
                }
            }
            // Through `Display`, symmetric with the `FromStr` decode in `from_row`.
            _ => {
                if is_opt {
                    quote! { #value::from(self.#ident.as_ref().map(::std::string::ToString::to_string)) }
                } else {
                    quote! { #value::from(self.#ident.to_string()) }
                }
            }
        }
    }

    /// The `OfflineRecord::from_row` initializer for one field; `decode` and `decode_msg` build its
    /// `OfflineDbError::Decode`.
    #[expect(clippy::too_many_lines, reason = "readability baseline")]
    pub fn offline_from_row_extraction(&self) -> proc_macro2::TokenStream {
        let ident = self.ident;
        let col_name = self
            .attrs
            .column
            .clone()
            .unwrap_or_else(|| ident.to_string());
        let is_opt = Self::is_option_type(self.ty);

        if let Some(inner) = Self::localized_inner(self.ty) {
            let core = crate::derives::core_path();
            return quote! {
                #ident: {
                    let __src = row
                        .get_named(#col_name)
                        .and_then(|v| v.as_text())
                        .ok_or_else(|| decode(#col_name))?;
                    #core::i18n::Localized::new(
                        __src.parse::<#inner>().map_err(|_| decode(#col_name))?,
                    )
                }
            };
        }

        // Decrypting needs a device data key not provisioned yet: an error, never ciphertext passed
        // off as plaintext. `Err(..)?` keeps the later initializers reachable.
        if Self::is_secret_type(self.ty) {
            let ty = self.ty;
            return quote! {
                #ident: ::core::result::Result::<#ty, _>::Err(decode_msg(
                    #col_name,
                    &"Secret column requires a device key to decrypt (not yet supported)",
                ))?
            };
        }

        // Each arm produces an `Option<FieldType>` (or `Option<Inner>` for `Option` fields).
        let extract: proc_macro2::TokenStream = if self.attrs.is_variant {
            let core = crate::derives::core_path();
            quote! { row.get_named(#col_name).and_then(|v| v.as_text()).and_then(|s| <_ as #core::Variant>::from_value(s)) }
        } else if self.attrs.is_enum {
            quote! { row.get_named(#col_name).and_then(|v| v.as_text()).and_then(|s| s.parse().ok()) }
        } else if Self::is_byte_vec(self.ty) {
            quote! { row.get_named(#col_name).and_then(|v| v.as_blob()).map(|b| b.to_vec()) }
        } else {
            match Self::terminal_ident(self.ty).as_deref() {
                Some("String" | "str") => {
                    quote! { row.get_named(#col_name).and_then(|v| v.as_text()).map(|s| s.to_string()) }
                }
                Some("bool") => quote! { row.get_named(#col_name).and_then(|v| v.as_bool()) },
                Some("i64") => quote! { row.get_named(#col_name).and_then(|v| v.as_integer()) },
                Some(
                    t @ ("i8" | "i16" | "i32" | "u8" | "u16" | "u32" | "u64" | "usize" | "isize"),
                ) => {
                    let tid = quote::format_ident!("{t}");
                    quote! { row.get_named(#col_name).and_then(|v| v.as_integer()).map(|i| i as #tid) }
                }
                Some("f64") => quote! { row.get_named(#col_name).and_then(|v| v.as_real()) },
                Some("f32") => {
                    quote! { row.get_named(#col_name).and_then(|v| v.as_real()).map(|f| f as f32) }
                }
                _ => {
                    quote! { row.get_named(#col_name).and_then(|v| v.as_text()).and_then(|s| s.parse().ok()) }
                }
            }
        };

        if is_opt {
            quote! { #ident: #extract }
        } else {
            quote! { #ident: #extract.ok_or_else(|| decode(#col_name))? }
        }
    }

    /// The offline `CREATE TABLE`; with `soft_delete`, a `deleted_at TEXT` column.
    pub fn generate_create_table_sql(
        table_name: &str,
        fields: &[FieldInfo<'_>],
        soft_delete: bool,
    ) -> String {
        let mut columns = Vec::new();
        let mut primary_key = None;

        for field in fields {
            let Some(spec) = field.column_spec(SchemaBackend::Sqlite) else {
                continue;
            };

            let mut col_def = format!("{} {}", spec.name, spec.storage_type);

            if spec.primary_key {
                col_def.push_str(" NOT NULL");
                primary_key = Some(spec.name.clone());
            } else if !spec.nullable {
                col_def.push_str(" NOT NULL");
            }

            columns.push(col_def);
        }

        if soft_delete {
            columns.push("deleted_at TEXT".to_string());
        }

        let mut sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (\n    {}",
            table_name,
            columns.join(",\n    ")
        );

        if let Some(pk) = primary_key {
            sql.push_str(&format!(",\n    PRIMARY KEY ({})", pk));
        }

        sql.push_str("\n)");
        sql
    }
}

impl<'a> FieldInfo<'a> {
    /// The text a closed value is stored as: a `Variant`'s value or an enum's `Display`.
    fn closed_value_text(&self) -> Option<proc_macro2::TokenStream> {
        let ident = self.ident;
        if self.attrs.is_variant {
            Some(Self::variant_value(
                self.ty,
                ident,
                &crate::derives::core_path(),
            ))
        } else if self.attrs.is_enum {
            Some(quote! { self.#ident.to_string() })
        } else {
            None
        }
    }

    /// A `Variant` field's stored value: its `value()` as a `String`, or an `Option<String>`.
    pub fn variant_value(
        ty: &Type,
        ident: &syn::Ident,
        core: &proc_macro2::TokenStream,
    ) -> proc_macro2::TokenStream {
        if Self::is_option_type(ty) {
            quote! {
                self.#ident
                    .as_ref()
                    .map(|__variant| ::std::borrow::ToOwned::to_owned(#core::Variant::value(__variant)))
            }
        } else {
            quote! { ::std::borrow::ToOwned::to_owned(#core::Variant::value(&self.#ident)) }
        }
    }

    /// Maps a Rust type to its YSQL type; unknown types, id newtypes included, are `TEXT`.
    pub fn rust_type_to_postgres(ty: &Type) -> &'static str {
        if Self::is_byte_vec(ty) {
            return "BYTEA";
        }
        // Secrets and `Localized` sources are TEXT, checked before the JSONB heuristic so a
        // `SecretJson` is stored as ciphertext, never JSONB.
        if Self::is_encrypted_column(ty) || Self::is_localized_type(ty) {
            return "TEXT";
        }
        let s = quote!(#ty).to_string();
        if s.contains("Vec < String >") || s.contains("Vec<String>") {
            return "JSONB";
        }
        if s.contains("serde_json") && s.contains("Value") {
            return "JSONB";
        }
        match Self::terminal_ident(ty).as_deref() {
            Some("String" | "str") => "TEXT",
            Some("bool") => "BOOL",
            Some("i16") => "INT2",
            Some("i32" | "u16") => "INT4",
            Some("i8" | "i64" | "u8" | "u32" | "u64" | "usize" | "isize") => "INT8",
            Some("f32") => "FLOAT4",
            Some("f64") => "FLOAT8",
            Some("Uuid") => "UUID",
            Some("DateTime") => "TIMESTAMPTZ",
            Some("NaiveDate") => "DATE",
            _ => "TEXT",
        }
    }

    /// A YSQL `CREATE TABLE`, with `deleted_at` under `soft_delete` and the sync overlay under
    /// `syncable`; the change-feed index is added through `ModelSchema`.
    pub fn generate_sql_create_table_sql(
        table_name: &str,
        fields: &[FieldInfo<'_>],
        soft_delete: bool,
        syncable: bool,
    ) -> String {
        let mut columns = Vec::new();
        // Every id column in declaration order, which is key order, each with its declared `shard`:
        // the engine picks one anyway, storing `PRIMARY KEY (a, b)` as `(a HASH, b ASC)`.
        let mut primary_key: Vec<String> = Vec::new();

        for field in fields {
            let Some(spec) = field.column_spec(SchemaBackend::Postgres) else {
                continue;
            };

            let mut col_def = format!("{} {}", spec.name, spec.storage_type);
            if spec.primary_key {
                col_def.push_str(" NOT NULL");
                primary_key.push(match spec.shard {
                    Some(shard) => format!("{} {}", spec.name, shard.as_sql()),
                    None => spec.name.clone(),
                });
            } else if !spec.nullable {
                col_def.push_str(" NOT NULL");
            }
            columns.push(col_def);
        }

        if soft_delete {
            columns.push("deleted_at TIMESTAMPTZ".to_string());
        }

        if syncable {
            columns.push("updated_at TIMESTAMPTZ NOT NULL DEFAULT now()".to_string());
            columns.push("version BIGINT NOT NULL DEFAULT 1".to_string());
            columns.push("_tombstone BOOL NOT NULL DEFAULT false".to_string());
        }

        let mut sql = format!(
            "CREATE TABLE IF NOT EXISTS {} (\n    {}",
            table_name,
            columns.join(",\n    ")
        );
        if !primary_key.is_empty() {
            sql.push_str(&format!(",\n    PRIMARY KEY ({})", primary_key.join(", ")));
        }
        sql.push_str("\n)");
        sql
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ty(s: &str) -> Type {
        syn::parse_str(s).expect("type parses")
    }

    #[test]
    fn secret_types_are_detected() {
        assert!(FieldInfo::is_secret_type(&ty("SecretString")));
        assert!(FieldInfo::is_secret_type(&ty("Secret<String>")));
        assert!(FieldInfo::is_secret_type(&ty("Secret<Vec<u8>>")));
        assert!(FieldInfo::is_secret_type(&ty("SecretBytes")));
        assert!(FieldInfo::is_secret_type(&ty("SecretJson<Profile>")));
        assert!(FieldInfo::is_secret_type(&ty("DatabaseUrl")));
        assert!(FieldInfo::is_secret_type(&ty(
            "fundamental_core::secrets::SecretString"
        )));
    }

    #[test]
    fn secret_subtypes_dispatch_distinctly() {
        assert!(FieldInfo::is_secret_bytes_type(&ty("SecretBytes")));
        assert!(!FieldInfo::is_secret_bytes_type(&ty("SecretString")));
        assert!(!FieldInfo::is_secret_bytes_type(&ty("SecretJson<Profile>")));

        assert!(FieldInfo::is_secret_json_type(&ty("SecretJson<Profile>")));
        assert!(!FieldInfo::is_secret_json_type(&ty("SecretString")));
        assert!(!FieldInfo::is_secret_json_type(&ty("SecretBytes")));

        let json_ty = ty("SecretJson<Profile>");
        let inner = FieldInfo::secret_json_inner(&json_ty);
        assert_eq!(quote!(#inner).to_string(), "Profile");
        let string_ty = ty("SecretString");
        assert!(FieldInfo::secret_json_inner(&string_ty).is_none());
    }

    #[test]
    fn non_secret_types_are_not_detected() {
        // Encrypted envelopes are ciphertext, not secrets.
        assert!(!FieldInfo::is_secret_type(&ty("EncryptedString")));
        assert!(!FieldInfo::is_secret_type(&ty("String")));
        assert!(!FieldInfo::is_secret_type(&ty("Option<i64>")));
        assert!(!FieldInfo::is_secret_type(&ty("Uuid")));
    }

    #[test]
    fn in_memory_secret_is_not_a_persisted_secret() {
        assert!(!FieldInfo::is_secret_type(&ty("InMemorySecret<String>")));
        assert!(!FieldInfo::is_secret_type(&ty("InMemorySecretString")));
        assert!(FieldInfo::is_in_memory_secret_type(&ty(
            "InMemorySecret<String>"
        )));
        assert!(FieldInfo::is_in_memory_secret_type(&ty(
            "InMemorySecretString"
        )));
        assert!(FieldInfo::is_in_memory_secret_type(&ty(
            "fundamental_core::secrets::InMemorySecret<Vec<u8>>"
        )));
    }

    #[test]
    fn persisted_secret_is_not_an_in_memory_secret() {
        assert!(!FieldInfo::is_in_memory_secret_type(&ty("SecretString")));
        assert!(!FieldInfo::is_in_memory_secret_type(&ty("Secret<String>")));
        assert!(!FieldInfo::is_in_memory_secret_type(&ty("DatabaseUrl")));
    }

    #[test]
    fn pluralize_handles_regular_rules() {
        assert_eq!(FieldInfo::pluralize("entry"), "entries"); // consonant + y → ies
        assert_eq!(FieldInfo::pluralize("day"), "days"); // vowel + y → s
        assert_eq!(FieldInfo::pluralize("box"), "boxes"); // x → es
        assert_eq!(FieldInfo::pluralize("class"), "classes"); // ss → es
        assert_eq!(FieldInfo::pluralize("dish"), "dishes"); // sh → es
        assert_eq!(FieldInfo::pluralize("record"), "records"); // default → s
        assert_eq!(FieldInfo::pluralize("keys"), "keys"); // already plural
    }

    #[test]
    fn pluralize_handles_f_to_ves_words() {
        assert_eq!(FieldInfo::pluralize("leaf"), "leaves");
        assert_eq!(FieldInfo::pluralize("knife"), "knives");
        assert_eq!(FieldInfo::pluralize("life"), "lives");
        assert_eq!(FieldInfo::pluralize("shelf"), "shelves");
        assert_eq!(FieldInfo::pluralize("wolf"), "wolves");
    }

    #[test]
    fn pluralize_leaves_irregular_f_exceptions_as_plain_s() {
        assert_eq!(FieldInfo::pluralize("roof"), "roofs");
        assert_eq!(FieldInfo::pluralize("chief"), "chiefs");
        assert_eq!(FieldInfo::pluralize("belief"), "beliefs");
        assert_eq!(FieldInfo::pluralize("safe"), "safes");
    }

    #[test]
    fn infer_table_name_pluralizes_only_the_last_word() {
        assert_eq!(
            FieldInfo::infer_table_name("OrganizationMember"),
            "organization_members"
        );
        assert_eq!(FieldInfo::infer_table_name("MerkleLeaf"), "merkle_leaves");
        assert_eq!(
            FieldInfo::infer_table_name("ResolutionCacheEntry"),
            "resolution_cache_entries"
        );
        assert_eq!(FieldInfo::infer_table_name("AuditOutbox"), "audit_outboxes");
    }
}
