//! Code generation for `#[derive(Model)]`.

use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Ident, Type};

use fundamental_core::model::cql::ClusteringOrder;

use super::{ColumnSpec, DatabaseBinding, FieldAttrs, FieldInfo, SchemaBackend, StructAttrs};

/// The prefix of every table the framework itself owns.
const RESERVED_TABLE_PREFIX: &str = "fundamental_";

/// Whether the crate being compiled is the framework's own, which alone may model a reserved
/// table; a hyphenated name (a compile-fail harness's `<package>-tests`) is a consumer.
fn compiling_the_framework() -> bool {
    std::env::var("CARGO_PKG_NAME")
        .is_ok_and(|name| name.starts_with("fundamental") && !name.contains('-'))
}

/// Whether `table` is spelled as the server resolves it: unquoted names fold case.
fn is_canonical_table_name(table: &str) -> bool {
    let mut bytes = table.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// The column a key field maps to, honouring `#[model(column = "…")]`.
fn key_column(field_infos: &[FieldInfo], id_field: Option<&Ident>) -> String {
    let Some(ident) = id_field else {
        return "id".to_string();
    };
    field_infos
        .iter()
        .find(|f| *f.ident == *ident)
        .and_then(|f| f.attrs.column.clone())
        .unwrap_or_else(|| ident.to_string())
}

/// The columns that address a syncable row, in placement order: YCQL leads with the partition
/// columns, YSQL with the `#[model(id)]` columns.
///
/// # Errors
/// Returns a `syn::Error` when the key does not contain the model's `#[model(id)]` column.
fn syncable_key_columns(
    ident: &Ident,
    struct_attrs: &StructAttrs,
    field_infos: &[FieldInfo],
    id_field: Option<&Ident>,
) -> syn::Result<Vec<String>> {
    let column_of = |f: &FieldInfo| {
        f.attrs
            .column
            .clone()
            .unwrap_or_else(|| f.ident.to_string())
    };
    let id_column = key_column(field_infos, id_field);

    let mut columns: Vec<String> = if struct_attrs.backends.cql {
        field_infos
            .iter()
            .filter(|f| f.attrs.partition_key)
            .map(&column_of)
            .chain(
                field_infos
                    .iter()
                    .filter(|f| f.attrs.clustering_key.is_some())
                    .map(&column_of),
            )
            .collect()
    } else {
        field_infos
            .iter()
            .filter(|f| f.attrs.is_id)
            .map(&column_of)
            .collect()
    };

    if columns.is_empty() {
        columns.push(id_column.clone());
    }

    if !columns.contains(&id_column) {
        return Err(syn::Error::new_spanned(
            ident,
            format!(
                "a syncable model's key must contain its `#[model(id)]` column \
                 `{id_column}`, but the key is ({}). On YCQL the id column must be \
                 one of the partition or clustering keys.",
                columns.join(", ")
            ),
        ));
    }
    Ok(columns)
}

/// The `translation_rows` and `apply_translation` methods for a model's `Localized` fields.
fn translation_row_methods(
    core: &proc_macro2::TokenStream,
    fields: &[&FieldInfo],
    columns: &[String],
) -> proc_macro2::TokenStream {
    let idents: Vec<&syn::Ident> = fields.iter().map(|f| f.ident).collect();
    let inners: Vec<&syn::Type> = fields
        .iter()
        .map(|f| FieldInfo::localized_inner(f.ty).expect("localized field"))
        .collect();
    quote! {
        fn translation_rows(&self) -> ::std::vec::Vec<#core::i18n::TranslationRow> {
            let mut __rows = ::std::vec::Vec::new();
            #(__rows.extend(#core::i18n::Localized::translation_rows(&self.#idents, #columns));)*
            __rows
        }
        fn apply_translation(&mut self, row: #core::i18n::TranslationRow) {
            match row.field.as_str() {
                #(#columns => {
                    if let ::core::result::Result::Ok(__v) = row.value.parse::<#inners>() {
                        #core::i18n::Localized::restore(&mut self.#idents, &row.locale, __v, row.check);
                    }
                })*
                _ => {}
            }
        }
    }
}

/// `Model::translation_gaps` over a model's `Localized` fields; nothing for a model with none.
fn translation_gaps_method(
    core: &proc_macro2::TokenStream,
    field_infos: &[FieldInfo],
) -> proc_macro2::TokenStream {
    let (idents, columns): (Vec<&syn::Ident>, Vec<String>) = field_infos
        .iter()
        .filter(|f| !f.attrs.skip && FieldInfo::is_localized_type(f.ty))
        .map(|f| {
            let column = f
                .attrs
                .column
                .clone()
                .unwrap_or_else(|| f.ident.to_string());
            (f.ident, column)
        })
        .unzip();
    if idents.is_empty() {
        return quote! {};
    }
    quote! {
        fn translation_gaps(
            &self,
            languages: &#core::i18n::Languages,
        ) -> ::std::vec::Vec<#core::i18n::TranslationGap> {
            let mut __gaps = ::std::vec::Vec::new();
            #(__gaps.extend(#core::i18n::TranslationGap::of(#columns, &self.#idents, languages));)*
            __gaps
        }
    }
}

fn connection_domain_expr(
    struct_attrs: &StructAttrs,
    core: &proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match &struct_attrs.connection {
        Some(path) => quote! { <#path as #core::model::Connection>::NAME },
        None => {
            let domain_str = struct_attrs
                .domain
                .clone()
                .unwrap_or_else(|| "default".to_string());
            quote! { #domain_str }
        }
    }
}

#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    reason = "readability baseline"
)]
pub(super) fn model_impl(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let core = crate::derives::core_path();
    // The backend crate path and the condition the online impls are emitted under
    // (`crate::derives::backend_impl_target`), so a build without the backend gets no impl of it.
    let sql_target = crate::derives::backend_impl_target("fundamental_sql", "sql");
    let cql_target = crate::derives::backend_impl_target("fundamental_cql", "cql");
    let sql = sql_target
        .as_ref()
        .map_or_else(crate::derives::sql_path, |(path, _)| path.clone());
    let cql = cql_target
        .as_ref()
        .map_or_else(crate::derives::cql_path, |(path, _)| path.clone());
    // `fundamental_db` is implied by every backend feature, `offline` included, so it is ungated.
    let db = crate::derives::db_path();
    let enc = crate::derives::encryption_path();
    let name = &input.ident;
    let type_name = name.to_string();

    let struct_attrs = StructAttrs::from_attrs(&input.attrs)?;
    if !struct_attrs.indexes.is_empty() && !struct_attrs.backends.sql {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "`#[model(index(...))]` declares a SQL secondary index; add `sql` to the model's backends",
        ));
    }

    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    input,
                    "Model can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "Model can only be derived for structs",
            ));
        }
    };

    let mut field_infos = Vec::new();
    let mut id_field: Option<&Ident> = None;
    let mut id_type: Option<&Type> = None;
    let mut id_fields: Vec<&Ident> = Vec::new();
    let mut id_types: Vec<&Type> = Vec::new();
    let mut owner_field: Option<&Ident> = None;

    for field in fields {
        let ident = field
            .ident
            .as_ref()
            .ok_or_else(|| syn::Error::new_spanned(field, "Field must have a name"))?;

        let mut attrs = FieldAttrs::from_attrs(&field.attrs)?;

        // An `InMemorySecret` is never persisted: it is treated as `#[model(skip)]`.
        if FieldInfo::is_in_memory_secret_type(&field.ty) {
            attrs.skip = true;
        }

        if attrs.no_sync && !struct_attrs.syncable {
            return Err(syn::Error::new_spanned(
                field,
                "`#[model(no_sync)]` requires the model to be `#[model(syncable)]`.",
            ));
        }

        if attrs.owner && !struct_attrs.syncable {
            return Err(syn::Error::new_spanned(
                field,
                "`#[model(owner)]` requires the model to be `#[model(syncable)]` \
                 (it names the per-principal column a slice scopes by).",
            ));
        }
        if attrs.owner {
            if owner_field.is_some() {
                return Err(syn::Error::new_spanned(
                    field,
                    "Only one field can be marked as #[model(owner)]",
                ));
            }
            owner_field = Some(ident);
        }

        // More than one id field is a composite key, in declaration order, which is key order.
        if attrs.is_id {
            if id_field.is_none() {
                id_field = Some(ident);
                id_type = Some(&field.ty);
            }
            id_fields.push(ident);
            id_types.push(&field.ty);
        }

        if attrs.encrypted && !FieldInfo::is_encrypted_column(&field.ty) {
            return Err(syn::Error::new_spanned(
                &field.ty,
                format!(
                    "Field '{}' is marked `#[model(encrypted)]` but is not a secret type.\n\n\
                     Use a Secret family type, which is encrypted at rest:\n  \
                     - SecretString  - for text data\n  \
                     - SecretBytes   - for binary data\n  \
                     - SecretJson<T> - for structured data\n  \
                     - Option<Secret*> - for a nullable secret (encrypted when present, NULL when absent)\n\n\
                     Example:\n  \
                     #[model(encrypted = \"pii\")]\n  \
                     ssn: SecretString,",
                    ident
                ),
            ));
        }

        field_infos.push(FieldInfo {
            ident,
            ty: &field.ty,
            attrs,
        });
    }

    // Persisted `Secret` fields with their key ids; `InMemorySecret` fields are already `skip`.
    let secret_fields: Vec<&FieldInfo> = field_infos
        .iter()
        .filter(|f| !f.attrs.skip && FieldInfo::is_encrypted_column(f.ty))
        .collect();
    let secret_col_names: Vec<String> = secret_fields
        .iter()
        .map(|f| {
            f.attrs
                .column
                .clone()
                .unwrap_or_else(|| f.ident.to_string())
        })
        .collect();
    let secret_col_keys: Vec<String> = secret_fields
        .iter()
        .map(|f| f.encryption_key_id())
        .collect();

    // The columns every encrypted field is sealed to, so a ciphertext decrypts only in its own row
    // while that row's owner and freshness are unchanged.
    let bound_fields: Vec<&FieldInfo> = field_infos
        .iter()
        .filter(|f| !f.attrs.skip && !FieldInfo::is_encrypted_column(f.ty) && !f.attrs.unbound)
        .collect();
    let bound_col_names: Vec<String> = bound_fields
        .iter()
        .map(|f| {
            f.attrs
                .column
                .clone()
                .unwrap_or_else(|| f.ident.to_string())
        })
        .collect();
    // The table, named once so the SQL and CQL impls, sync and every row binding agree.
    let row_table_name = struct_attrs
        .table
        .clone()
        .unwrap_or_else(|| FieldInfo::infer_table_name(&type_name));
    // `fundamental_*` tables are the framework's ledgers: refused outside the framework, and not an
    // `EffectModel` inside it, so an effect cannot rewrite the ledger that records it.
    if !is_canonical_table_name(&row_table_name) {
        return Err(syn::Error::new_spanned(
            name,
            format!(
                "the table `{row_table_name}` is not a table name as the database resolves it: \
                 lower-case letters, digits and `_`, starting with a letter or `_`"
            ),
        ));
    }
    let framework_table = row_table_name.starts_with(RESERVED_TABLE_PREFIX);
    if framework_table && !compiling_the_framework() {
        return Err(syn::Error::new_spanned(
            name,
            format!(
                "the table `{row_table_name}` is reserved: `{RESERVED_TABLE_PREFIX}*` tables belong to the framework"
            ),
        ));
    }

    for f in &field_infos {
        let is_key = f.attrs.is_id || f.attrs.partition_key || f.attrs.clustering_key.is_some();
        if f.attrs.unbound && (is_key || FieldInfo::is_encrypted_column(f.ty)) {
            return Err(syn::Error::new_spanned(
                f.ident,
                "`#[model(unbound)]` names a bookkeeping column to leave out of the row binding; \
                 a key column and an encrypted column cannot be unbound",
            ));
        }
        if is_key && !f.attrs.skip && FieldInfo::is_encrypted_column(f.ty) {
            return Err(syn::Error::new_spanned(
                f.ty,
                "An encrypted `Secret` field cannot be a key column: it is sealed with a fresh nonce \
                 on every write, so a row could never be addressed by it",
            ));
        }
    }
    for f in field_infos.iter().filter(|f| f.attrs.erasable) {
        if !struct_attrs.outbox {
            return Err(syn::Error::new_spanned(
                f.ident,
                "`#[model(erasable)]` names a column an erasure may clear, which only an outbox seal \
                 leaves out; add `#[model(outbox)]` to the struct or remove it",
            ));
        }
        let is_key = f.attrs.is_id || f.attrs.partition_key || f.attrs.clustering_key.is_some();
        if is_key
            || f.attrs.skip
            || f.attrs.unbound
            || FieldInfo::is_encrypted_column(f.ty)
            || !FieldInfo::is_option_type(f.ty)
        {
            return Err(syn::Error::new_spanned(
                f.ident,
                "`#[model(erasable)]` names a nullable, non-key, non-secret column an erasure clears; \
                 a key column, a skipped, unbound or encrypted column, or one that is not an `Option` \
                 cannot be erasable",
            ));
        }
    }
    if struct_attrs.syncable && !secret_fields.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "A `#[model(syncable)]` model cannot carry encrypted `Secret` fields: a device can never \
             seal a value to its row, so accepting an envelope from one only lets a replayed one in",
        ));
    }

    let secret_columns_method = if secret_fields.is_empty() {
        quote! {}
    } else {
        quote! {
            fn secret_columns() -> &'static [(&'static str, &'static str)] {
                &[#((#secret_col_names, #secret_col_keys)),*]
            }

            fn row_bound_columns() -> &'static [&'static str] {
                &[#(#bound_col_names),*]
            }
        }
    };

    // An outbox seal covers the key and every bound column an erasure never clears.
    let sealed_fields: Vec<&FieldInfo> = bound_fields
        .iter()
        .copied()
        .filter(|f| !f.attrs.erasable)
        .collect();
    let sealed_col_names: Vec<String> = sealed_fields
        .iter()
        .map(|f| {
            f.attrs
                .column
                .clone()
                .unwrap_or_else(|| f.ident.to_string())
        })
        .collect();
    let outbox_sealed_columns_method = if struct_attrs.outbox {
        quote! {
            fn outbox_sealed_columns() -> ::core::option::Option<&'static [&'static str]> {
                ::core::option::Option::Some(&[#(#sealed_col_names),*])
            }
        }
    } else {
        quote! {}
    };
    let sealed_outbox_impl = if struct_attrs.outbox {
        quote! { impl #core::outbox::SealedOutbox for #name {} }
    } else {
        quote! {}
    };

    // Server-side sync metadata, backend-neutral; which rows a device syncs is decided by slices.
    // Shared by `ServerSyncable` and `Syncable`: a mirror keyed differently from its rows could not
    // address what it receives.
    let syncable_key_columns: Vec<String> = if struct_attrs.syncable {
        syncable_key_columns(&input.ident, &struct_attrs, &field_infos, id_field)?
    } else {
        Vec::new()
    };

    let server_syncable_impl = if struct_attrs.syncable {
        let online_table = row_table_name.clone();
        let bidirectional = struct_attrs.bidirectional;
        // Every persisted, syncable field except the key columns, which travel in the record's key;
        // a `Secret` travels as its ciphertext. A key column in the payload too would let a push
        // clear the slice check on one owner and write the row under another.
        let data_cols: Vec<String> = field_infos
            .iter()
            .filter(|f| !f.attrs.skip && !f.attrs.no_sync)
            .map(|f| {
                f.attrs
                    .column
                    .clone()
                    .unwrap_or_else(|| f.ident.to_string())
            })
            .filter(|column| !syncable_key_columns.contains(column))
            .collect();
        // `None` for an unscoped table: servable on YSQL, not by YCQL's per-owner outbox.
        let owner_col = field_infos.iter().find(|f| f.attrs.owner).map(|f| {
            f.attrs
                .column
                .clone()
                .unwrap_or_else(|| f.ident.to_string())
        });
        let owner_column = match owner_col {
            Some(col) => quote! { Some(#col) },
            None => quote! { None },
        };
        let key_cols = &syncable_key_columns;
        // The leading prefix of the key: the YCQL partition columns, or the whole key elsewhere.
        let partition_cols: Vec<String> = if struct_attrs.backends.cql {
            field_infos
                .iter()
                .filter(|f| f.attrs.partition_key)
                .map(|f| {
                    f.attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string())
                })
                .collect()
        } else {
            key_cols.clone()
        };
        let single_key = key_cols.len() == 1;
        let first_key_col = key_cols.first().cloned().unwrap_or_default();
        let sync = crate::derives::sync_path();
        let single_key_syncable_impl = if single_key {
            quote! { impl #sync::SingleKeySyncable for #name { const KEY_COLUMN: &'static str = #first_key_col; } }
        } else {
            quote! {}
        };
        quote! {
            impl #sync::ServerSyncable for #name {
                const TABLE: &'static str = #online_table;
                const KEY_COLUMNS: &'static [&'static str] = &[#(#key_cols),*];
                const PARTITION_COLUMNS: &'static [&'static str] = &[#(#partition_cols),*];
                const BIDIRECTIONAL: bool = #bidirectional;
                const OWNER_COLUMN: Option<&'static str> = #owner_column;
                fn data_columns() -> &'static [&'static str] {
                    &[#(#data_cols),*]
                }
            }

            #single_key_syncable_impl
        }
    } else {
        quote! {}
    };

    let partition_key_fields: Vec<_> = field_infos
        .iter()
        .filter(|f| f.attrs.partition_key)
        .collect();

    let clustering_key_fields: Vec<_> = field_infos
        .iter()
        .filter(|f| f.attrs.clustering_key.is_some())
        .collect();

    let column_constants: Vec<proc_macro2::TokenStream> = field_infos
        .iter()
        .filter(|f| !f.attrs.skip)
        .map(|f| {
            let col_name = f
                .attrs
                .column
                .clone()
                .unwrap_or_else(|| f.ident.to_string());
            let const_name = Ident::new(&f.ident.to_string().to_uppercase(), f.ident.span());
            let doc = format!("The name of the `{col_name}` column.");
            quote! {
                #[doc = #doc]
                pub const #const_name: &'static str = #col_name;
            }
        })
        .collect();

    // i18n labels for every model, computed before the backend code below moves `struct_attrs`.
    let labels_impl = generate_labels_impl(&core, name, &type_name, &struct_attrs, &field_infos);

    // The base `Model` impl, for every backend; the key accessor lives on `SqlModel`, and YCQL's
    // composite key in its `CqlModel` machinery.
    let translation_gaps_method = translation_gaps_method(&core, &field_infos);
    let model_impl = quote! {
        impl #core::model::Model for #name {
            const TYPE_NAME: &'static str = #type_name;
            #secret_columns_method
            #outbox_sealed_columns_method
            #translation_gaps_method
        }

        #sealed_outbox_impl
    };

    let database = if struct_attrs.backends.sql {
        Some(DatabaseBinding::SqlDb)
    } else if struct_attrs.backends.cql {
        Some(DatabaseBinding::CqlDb)
    } else {
        None
    };

    // Resolved once: both the backend block and the `connection = C` touch, emitted under its
    // complement, depend on it.
    let backend_emit = match database {
        Some(DatabaseBinding::SqlDb) => BackendEmit::of(&sql_target, "sql"),
        Some(DatabaseBinding::CqlDb) => BackendEmit::of(&cql_target, "cql"),
        None => BackendEmit::Never,
    };

    let database_impl = match database {
        Some(DatabaseBinding::SqlDb) => {
            let table_name = row_table_name.clone();

            let primary_key = key_column(&field_infos, id_field);

            let domain_expr = connection_domain_expr(&struct_attrs, &core);

            // `Localized` fields: the source value inline, other locales in a `<table>_i18n`
            // companion.
            let cr_localized_fields: Vec<&FieldInfo> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && FieldInfo::is_localized_type(f.ty))
                .collect();
            let cr_has_localized = !cr_localized_fields.is_empty();
            let cr_localized_columns: Vec<String> = cr_localized_fields
                .iter()
                .map(|f| {
                    f.attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string())
                })
                .collect();
            let cr_translation_table = format!("{table_name}_i18n");
            let soft_delete = struct_attrs.soft_delete;
            let syncable = struct_attrs.syncable;
            let cr_main_ddl = FieldInfo::generate_sql_create_table_sql(
                &table_name,
                &field_infos,
                soft_delete,
                syncable,
            );
            let cr_translation_methods = if cr_has_localized {
                let cols = &cr_localized_columns;
                let row_methods = translation_row_methods(&core, &cr_localized_fields, cols);
                quote! {
                    fn translatable_columns() -> &'static [&'static str] { &[#(#cols),*] }
                    fn translation_table() -> ::core::option::Option<&'static str> {
                        ::core::option::Option::Some(#cr_translation_table)
                    }
                    #row_methods
                }
            } else {
                quote! {}
            };

            // A model with a `Secret` gets panicking plain codecs; the real work is in
            // `*_encrypted`.
            let has_secret = field_infos
                .iter()
                .any(|f| !f.attrs.skip && FieldInfo::is_encrypted_column(f.ty));
            // An outbox needs the key manager too, to seal each row it writes and check each it
            // reads.
            let is_outbox = struct_attrs.outbox;
            let needs_key_manager = has_secret || is_outbox;

            let gen_from_row_value = |f: &FieldInfo| -> proc_macro2::TokenStream {
                let col_name = f
                    .attrs
                    .column
                    .clone()
                    .unwrap_or_else(|| f.ident.to_string());
                let field_ty = f.ty;
                let type_str = quote!(#field_ty).to_string();

                // `Vec<String>` is JSONB and decoded explicitly; everything else through sqlx's
                // `try_get`.
                let is_vec_string =
                    type_str.contains("Vec < String >") || type_str.contains("Vec<String>");

                if let Some(inner) = FieldInfo::localized_inner(field_ty) {
                    quote! {
                        {
                            let __src: String = #sql::sqlx::Row::try_get(row, #col_name)?;
                            #core::i18n::Localized::new(
                                __src.parse::<#inner>().map_err(|_| #sql::SqlError::RowRefused {
                                    table: #row_table_name,
                                    column: #col_name,
                                    kind: #core::outbox::RefusalKind::ValueRefused,
                                })?
                            )
                        }
                    }
                } else if f.attrs.is_variant {
                    // A value no variant has is the row's refusal, naming its column.
                    let refused = quote! {
                        #sql::SqlError::RowRefused {
                            table: #row_table_name,
                            column: #col_name,
                            kind: #core::outbox::RefusalKind::ValueRefused,
                        }
                    };
                    if FieldInfo::is_option_type(field_ty) {
                        quote! {
                            {
                                let __val: Option<String> = #sql::sqlx::Row::try_get_unchecked(row, #col_name)?;
                                __val
                                    .map(|__val| <_ as #core::Variant>::from_value(&__val).ok_or(#refused))
                                    .transpose()?
                            }
                        }
                    } else {
                        quote! {
                            {
                                let __val: String = #sql::sqlx::Row::try_get_unchecked(row, #col_name)?;
                                <_ as #core::Variant>::from_value(&__val).ok_or(#refused)?
                            }
                        }
                    }
                } else if f.attrs.is_enum {
                    // Decoded as a String and parsed, since sqlx cannot know a custom enum type's
                    // OID.
                    quote! {
                        {
                            let __val: String = #sql::sqlx::Row::try_get_unchecked(row, #col_name)?;
                            __val.parse().map_err(|_: String| #sql::SqlError::RowRefused {
                                table: #row_table_name,
                                column: #col_name,
                                kind: #core::outbox::RefusalKind::ValueRefused,
                            })?
                        }
                    }
                } else if is_vec_string {
                    // Malformed JSONB is an error naming the column, not an empty Vec.
                    quote! {
                        {
                            let json: serde_json::Value = #sql::sqlx::Row::try_get(row, #col_name)?;
                            serde_json::from_value(json).map_err(|_| #sql::SqlError::RowRefused {
                                table: #row_table_name,
                                column: #col_name,
                                kind: #core::outbox::RefusalKind::ValueRefused,
                            })?
                        }
                    }
                } else {
                    // A value the type cannot take is the row's refusal; a missing column keeps the
                    // driver's error.
                    quote! {
                        #sql::sqlx::Row::try_get(row, #col_name).map_err(|error| match error {
                            #sql::sqlx::Error::ColumnDecode { .. } => #sql::SqlError::RowRefused {
                                table: #row_table_name,
                                column: #col_name,
                                kind: #core::outbox::RefusalKind::ValueRefused,
                            },
                            other => #sql::SqlError::from(other),
                        })?
                    }
                }
            };
            let from_row_fields: Vec<_> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip)
                .map(|f| {
                    let field_ident = f.ident;
                    let value = gen_from_row_value(f);
                    quote! { #field_ident: #value }
                })
                .collect();

            // The `from_row_encrypted` initializer: a `Secret` column decrypts through `enc` by
            // subtype; the key id is read from the envelope.
            let gen_secret_from_row_value = |f: &FieldInfo| -> proc_macro2::TokenStream {
                {
                    let optional = FieldInfo::optional_secret_inner(f.ty).is_some();
                    let col_name = f
                        .attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string());
                    let decrypt = if FieldInfo::is_secret_bytes_type(f.ty) {
                        quote! {
                            let __es: #enc::EncryptedBytes = #enc::decode_envelope(&__env)
                                .map_err(|_| #sql::SqlError::RowRefused {
                                    table: #row_table_name,
                                    column: #col_name,
                                    kind: #core::outbox::RefusalKind::EnvelopeRefused,
                                })?;
                            let __pt = __km.decrypt_bytes(&__es, &#enc::RowBinding::new(#row_table_name, #col_name, &__row_parts))
                                .map_err(|error| match error.refusal() {
                                    ::core::option::Option::Some(kind) => #sql::SqlError::RowRefused {
                                        table: #row_table_name,
                                        column: #col_name,
                                        kind,
                                    },
                                    ::core::option::Option::None => #sql::SqlError::Other(#core::AnyError::new(error)),
                                })?;
                        }
                    } else if let Some(inner) = FieldInfo::secret_json_inner(f.ty) {
                        quote! {
                            let __es: #enc::EncryptedString = #enc::decode_envelope(&__env)
                                .map_err(|_| #sql::SqlError::RowRefused {
                                    table: #row_table_name,
                                    column: #col_name,
                                    kind: #core::outbox::RefusalKind::EnvelopeRefused,
                                })?;
                            let __pt = __km.decrypt_string_to_json::<#inner>(&__es, &#enc::RowBinding::new(#row_table_name, #col_name, &__row_parts))
                                .map_err(|error| match error.refusal() {
                                    ::core::option::Option::Some(kind) => #sql::SqlError::RowRefused {
                                        table: #row_table_name,
                                        column: #col_name,
                                        kind,
                                    },
                                    ::core::option::Option::None => #sql::SqlError::Other(#core::AnyError::new(error)),
                                })?;
                        }
                    } else {
                        quote! {
                            let __es: #enc::EncryptedString = #enc::decode_envelope(&__env)
                                .map_err(|_| #sql::SqlError::RowRefused {
                                    table: #row_table_name,
                                    column: #col_name,
                                    kind: #core::outbox::RefusalKind::EnvelopeRefused,
                                })?;
                            let __pt = __km.decrypt_string(&__es, &#enc::RowBinding::new(#row_table_name, #col_name, &__row_parts))
                                .map_err(|error| match error.refusal() {
                                    ::core::option::Option::Some(kind) => #sql::SqlError::RowRefused {
                                        table: #row_table_name,
                                        column: #col_name,
                                        kind,
                                    },
                                    ::core::option::Option::None => #sql::SqlError::Other(#core::AnyError::new(error)),
                                })?;
                        }
                    };
                    if optional {
                        // A NULL column is `None` with no key needed.
                        quote! {
                            {
                                let __env: ::core::option::Option<String> =
                                    #sql::sqlx::Row::try_get(row, #col_name)?;
                                match __env {
                                    ::core::option::Option::Some(__env) => {
                                        let __km = enc.ok_or_else(|| #sql::SqlError::Query(
                                            format!("secret column '{}' needs a key manager to decrypt", #col_name)))?;
                                        #decrypt
                                        ::core::option::Option::Some(#core::secrets::Secret::new(__pt))
                                    }
                                    ::core::option::Option::None => ::core::option::Option::None,
                                }
                            }
                        }
                    } else {
                        quote! {
                            {
                                let __km = enc.ok_or_else(|| #sql::SqlError::Query(
                                    format!("secret column '{}' needs a key manager to decrypt", #col_name)))?;
                                let __env: String = #sql::sqlx::Row::try_get(row, #col_name)?;
                                #decrypt
                                #core::secrets::Secret::new(__pt)
                            }
                        }
                    }
                }
            };
            // Non-secret columns first: the row binding is computed from them before a secret can
            // open.
            let local_of =
                |f: &FieldInfo| Ident::new(&format!("__col_{}", f.ident), f.ident.span());
            let from_row_plain_locals: Vec<_> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && !FieldInfo::is_encrypted_column(f.ty))
                .map(|f| {
                    let local = local_of(f);
                    let value = gen_from_row_value(f);
                    quote! { let #local = #value; }
                })
                .collect();
            let from_row_bound_parts: Vec<_> = bound_fields
                .iter()
                .map(|f| {
                    let local = local_of(f);
                    {
                    let name = f.attrs.column.clone().unwrap_or_else(|| f.ident.to_string());
                    quote! { (#name, #core::model::RowBound::row_part(&#local, #core::model::TimestampPrecision::Microseconds)) }
                    }
                })
                .collect();
            let from_row_secret_locals: Vec<_> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && FieldInfo::is_encrypted_column(f.ty))
                .map(|f| {
                    let local = local_of(f);
                    let value = gen_secret_from_row_value(f);
                    quote! { let #local = #value; }
                })
                .collect();
            let from_row_fields_from_locals: Vec<_> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip)
                .map(|f| {
                    let field_ident = f.ident;
                    let local = local_of(f);
                    quote! { #field_ident: #local }
                })
                .collect();

            let from_row_skipped: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| f.attrs.skip)
                .map(|f| {
                    let field_ident = f.ident;
                    quote! { #field_ident: ::core::default::Default::default() }
                })
                .collect();

            let secret_plain_panic = if has_secret {
                format!(
                    "{type_name} has encrypted Secret columns; persist it through the \
                     key-manager-backed repository, not the plain codec"
                )
            } else {
                format!(
                    "{type_name} is an outbox whose rows are sealed; persist it through the \
                     key-manager-backed repository, not the plain codec"
                )
            };

            // Before any secret opens, so a planted or edited row refuses as a row, never as a
            // relay.
            let from_row_seal_parts: Vec<_> = sealed_fields
                .iter()
                .map(|f| {
                    let local = local_of(f);
                    let name = f.attrs.column.clone().unwrap_or_else(|| f.ident.to_string());
                    quote! { (#name, #core::model::RowBound::row_part(&#local, #core::model::TimestampPrecision::Microseconds)) }
                })
                .collect();
            let from_row_seal_check = if is_outbox {
                quote! {
                    {
                        let __km = enc.ok_or_else(|| #sql::SqlError::Query(::std::format!(
                            "{} rows are sealed and need a key manager to read", #type_name)))?;
                        let __refused = |kind: #core::outbox::RefusalKind| #sql::SqlError::RowRefused {
                            table: #row_table_name,
                            column: #core::outbox::OutboxColumns::SEAL,
                            kind,
                        };
                        let __stored: ::core::option::Option<::std::vec::Vec<u8>> =
                            #sql::sqlx::Row::try_get(row, #core::outbox::OutboxColumns::SEAL)?;
                        let ::core::option::Option::Some(__stored) = __stored else {
                            return ::core::result::Result::Err(__refused(#core::outbox::RefusalKind::EnvelopeRefused));
                        };
                        let __seal_parts: ::std::vec::Vec<(&'static str, ::core::option::Option<::std::vec::Vec<u8>>)> =
                            ::std::vec![#(#from_row_seal_parts),*];
                        __km.check_outbox_seal(&__stored, #row_table_name, &__seal_parts)
                            .map_err(|error| match error.refusal() {
                                ::core::option::Option::Some(kind) => __refused(kind),
                                ::core::option::Option::None => #sql::SqlError::Other(#core::AnyError::new(error)),
                            })?;
                    }
                }
            } else {
                quote! {}
            };
            let from_row_row_parts = if has_secret {
                quote! {
                    let __row_parts: ::std::vec::Vec<(&'static str, ::core::option::Option<::std::vec::Vec<u8>>)> =
                        ::std::vec![#(#from_row_bound_parts),*];
                }
            } else {
                quote! {}
            };

            let from_row_impl = if struct_attrs.no_from_row {
                quote! {}
            } else if needs_key_manager {
                // Without a key manager a secret-bearing row is a typed error on the read path.
                quote! {
                    impl #sql::FromRow for #name {
                        /// This model has `Secret` columns, which need a key manager to decrypt,
                        /// so a row holding a value is an error here; read it through the
                        /// repository, which calls `from_row_encrypted`.
                        fn from_row(row: &#sql::sqlx::postgres::PgRow) -> #sql::SqlResult<Self> {
                            Self::from_row_encrypted(row, ::core::option::Option::None)
                        }

                        fn from_row_encrypted(
                            row: &#sql::sqlx::postgres::PgRow,
                            enc: ::core::option::Option<&#enc::KeyManager>,
                        ) -> #sql::SqlResult<Self> {
                            use #sql::sqlx::Row as _;
                            #(#from_row_plain_locals)*
                            #from_row_row_parts
                            #from_row_seal_check
                            #(#from_row_secret_locals)*
                            Ok(Self {
                                #(#from_row_fields_from_locals,)*
                                #(#from_row_skipped,)*
                            })
                        }
                    }
                }
            } else {
                quote! {
                    impl #sql::FromRow for #name {
                        fn from_row(row: &#sql::sqlx::postgres::PgRow) -> #sql::SqlResult<Self> {
                            use #sql::sqlx::Row as _;
                            Ok(Self {
                                #(#from_row_fields,)*
                                #(#from_row_skipped,)*
                            })
                        }
                    }
                }
            };

            let all_col_names: Vec<String> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip)
                .map(|f| {
                    f.attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string())
                })
                .collect();

            let non_pk_col_names: Vec<String> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && !f.attrs.is_id)
                .map(|f| {
                    f.attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string())
                })
                .collect();

            let gen_bind_expr = |f: &FieldInfo| -> proc_macro2::TokenStream {
                let ident = f.ident;
                let field_ty = f.ty;
                let type_str = quote!(#field_ty).to_string();
                let is_vec_string =
                    type_str.contains("Vec < String >") || type_str.contains("Vec<String>");
                let is_json_value = type_str.contains("Value") && type_str.contains("serde_json");

                if FieldInfo::is_localized_type(field_ty) {
                    quote! {
                        let query = query.bind_value(
                            ::std::string::ToString::to_string(#core::i18n::Localized::source(&self.#ident))
                        );
                    }
                } else if f.attrs.is_variant {
                    let value = FieldInfo::variant_value(field_ty, ident, &core);
                    quote! { let query = query.bind_value(#value); }
                } else if f.attrs.is_enum {
                    quote! { let query = query.bind_value(self.#ident.to_string()); }
                } else if is_vec_string {
                    quote! { let query = query.bind_value(serde_json::json!(self.#ident)); }
                } else if is_json_value {
                    // serde_json::Value binds directly, as the fallback does.
                    quote! { let query = query.bind_value(self.#ident.clone()); }
                } else {
                    quote! { let query = query.bind_value(self.#ident.clone()); }
                }
            };

            let bind_all_fields: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip)
                .map(&gen_bind_expr)
                .collect();

            let bind_non_pk_fields: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && !f.attrs.is_id)
                .map(&gen_bind_expr)
                .collect();
            // The key columns in key order, the complement of the list above, for an UPDATE's
            // WHERE.
            let bind_key_fields: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && f.attrs.is_id)
                .map(&gen_bind_expr)
                .collect();

            // The `bind_value` argument for a single field, mirroring `gen_bind_expr`.
            let gen_bind_value = |f: &FieldInfo| -> proc_macro2::TokenStream {
                let ident = f.ident;
                let field_ty = f.ty;
                let type_str = quote!(#field_ty).to_string();
                let is_vec_string =
                    type_str.contains("Vec < String >") || type_str.contains("Vec<String>");

                if FieldInfo::is_localized_type(field_ty) {
                    quote! { ::std::string::ToString::to_string(#core::i18n::Localized::source(&self.#ident)) }
                } else if f.attrs.is_variant {
                    FieldInfo::variant_value(field_ty, ident, &core)
                } else if f.attrs.is_enum {
                    quote! { self.#ident.to_string() }
                } else if is_vec_string {
                    quote! { serde_json::json!(self.#ident) }
                } else {
                    quote! { self.#ident.clone() }
                }
            };

            // A `Secret` column binds its ciphertext envelope, encrypted through `enc`; the `?`
            // propagates from the enclosing `*_encrypted` method.
            let gen_bind_value_encrypted = |f: &FieldInfo| -> proc_macro2::TokenStream {
                if FieldInfo::is_encrypted_column(f.ty) {
                    let ident = f.ident;
                    let optional = FieldInfo::optional_secret_inner(f.ty).is_some();
                    let col = f
                        .attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string());
                    let key_id = f.encryption_key_id();
                    // `__secret` is the exposed plaintext: `&self.#ident`, or the unwrapped `Some`
                    // when nullable.
                    let encrypt = if FieldInfo::is_secret_bytes_type(f.ty) {
                        quote! { __km.encrypt_bytes(__secret.expose(), #key_id, &#enc::RowBinding::new(#row_table_name, #col, &__row_parts)) }
                    } else if FieldInfo::is_secret_json_type(f.ty) {
                        quote! { __km.encrypt_json_to_string(__secret.expose(), #key_id, &#enc::RowBinding::new(#row_table_name, #col, &__row_parts)) }
                    } else {
                        quote! { __km.encrypt_string(__secret.expose(), #key_id, &#enc::RowBinding::new(#row_table_name, #col, &__row_parts)) }
                    };
                    let envelope = quote! {
                        {
                            let __km = enc.ok_or_else(|| #sql::SqlError::Config(
                                format!("model column '{}' is an encrypted Secret but no key manager was provided", #col)))?;
                            let __ct = #encrypt
                                .map_err(|e| #sql::SqlError::Config(
                                    format!("encrypt secret column '{}' failed: {}", #col, e)))?;
                            #enc::encode_envelope(&__ct)
                        }
                    };
                    if optional {
                        // `NULL` when absent; persisting a `None` needs no key.
                        quote! {
                            match self.#ident.as_ref() {
                                ::core::option::Option::Some(__secret) => ::core::option::Option::Some(#envelope),
                                ::core::option::Option::None => ::core::option::Option::None,
                            }
                        }
                    } else {
                        quote! {
                            {
                                let __secret = &self.#ident;
                                #envelope
                            }
                        }
                    }
                } else {
                    gen_bind_value(f)
                }
            };

            // `bind_column_for_batch` arms in `columns_without_pk()` order, for `update_many`.
            let bind_batch_arms: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && !f.attrs.is_id)
                .enumerate()
                .map(|(i, f)| {
                    let value = gen_bind_value(f);
                    quote! { #i => query.bind_value(#value), }
                })
                .collect();

            let bind_all_values_enc: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip)
                .map(&gen_bind_value_encrypted)
                .collect();
            let bind_non_pk_values_enc: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && !f.attrs.is_id)
                .map(&gen_bind_value_encrypted)
                .collect();
            // The key columns' values in key order; with `bind_without_pk_encrypted` each column
            // binds once.
            let bind_key_values_enc: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && f.attrs.is_id)
                .map(&gen_bind_value_encrypted)
                .collect();
            let bind_batch_arms_enc: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && !f.attrs.is_id)
                .enumerate()
                .map(|(i, f)| {
                    let value = gen_bind_value_encrypted(f);
                    quote! { #i => ::core::result::Result::Ok(query.bind_value(#value)), }
                })
                .collect();

            // Only a model with encrypted columns computes the row binding; its column types need
            // not otherwise be bindable.
            let bound_parts_from_self: Vec<_> = bound_fields
                .iter()
                .map(|f| {
                    let ident = f.ident;
                    {
                    let name = f.attrs.column.clone().unwrap_or_else(|| f.ident.to_string());
                    quote! { (#name, #core::model::RowBound::row_part(&self.#ident, #core::model::TimestampPrecision::Microseconds)) }
                    }
                })
                .collect();
            let seal_parts_from_self: Vec<_> = sealed_fields
                .iter()
                .map(|f| {
                    let ident = f.ident;
                    let name = f.attrs.column.clone().unwrap_or_else(|| f.ident.to_string());
                    quote! { (#name, #core::model::RowBound::row_part(&self.#ident, #core::model::TimestampPrecision::Microseconds)) }
                })
                .collect();
            let seal_value = quote! {
                {
                    let __km = enc.ok_or_else(|| #sql::SqlError::Config(::std::format!(
                        "{} is an outbox whose rows are sealed, but no key manager was provided", #type_name)))?;
                    let __seal_parts: ::std::vec::Vec<(&'static str, ::core::option::Option<::std::vec::Vec<u8>>)> =
                        ::std::vec![#(#seal_parts_from_self),*];
                    __km.seal_outbox_row(#row_table_name, &__seal_parts)
                        .map_err(|error| #sql::SqlError::Config(::std::format!(
                            "sealing a {} row failed: {}", #row_table_name, error)))?
                        .to_vec()
                }
            };
            let (bind_seal, seal_column, seal_batch_arm) = if is_outbox {
                let seal_index = field_infos
                    .iter()
                    .filter(|f| !f.attrs.skip && !f.attrs.is_id)
                    .count();
                (
                    quote! { let query = query.bind_value(#seal_value); },
                    quote! { #core::outbox::OutboxColumns::SEAL },
                    quote! { #seal_index => ::core::result::Result::Ok(query.bind_value(#seal_value)), },
                )
            } else {
                (quote! {}, quote! {}, quote! {})
            };
            let row_parts_from_self = if has_secret {
                quote! {
                    let __row_parts: ::std::vec::Vec<(&'static str, ::core::option::Option<::std::vec::Vec<u8>>)> =
                        ::std::vec![#(#bound_parts_from_self),*];
                }
            } else {
                quote! {}
            };

            let encrypted_bind_methods = quote! {
                fn bind_to_encrypted<'q, Q>(
                    &'q self,
                    query: Q,
                    enc: ::core::option::Option<&#enc::KeyManager>,
                ) -> #sql::SqlResult<Q>
                where
                    Q: #sql::BindValues<'q>,
                {
                    #row_parts_from_self
                    #(let query = query.bind_value(#bind_all_values_enc);)*
                    #bind_seal
                    ::core::result::Result::Ok(query)
                }

                fn bind_without_pk_encrypted<'q, Q>(
                    &'q self,
                    query: Q,
                    enc: ::core::option::Option<&#enc::KeyManager>,
                ) -> #sql::SqlResult<Q>
                where
                    Q: #sql::BindValues<'q>,
                {
                    #row_parts_from_self
                    #(let query = query.bind_value(#bind_non_pk_values_enc);)*
                    #bind_seal
                    ::core::result::Result::Ok(query)
                }

                fn bind_key_encrypted<'q, Q>(
                    &'q self,
                    query: Q,
                    enc: ::core::option::Option<&#enc::KeyManager>,
                ) -> #sql::SqlResult<Q>
                where
                    Q: #sql::BindValues<'q>,
                {
                    #(let query = query.bind_value(#bind_key_values_enc);)*
                    ::core::result::Result::Ok(query)
                }

                fn bind_column_for_batch_encrypted<'q, Q>(
                    &'q self,
                    query: Q,
                    column_index: usize,
                    enc: ::core::option::Option<&#enc::KeyManager>,
                ) -> #sql::SqlResult<Q>
                where
                    Q: #sql::BindValues<'q>,
                {
                    #row_parts_from_self
                    match column_index {
                        #(#bind_batch_arms_enc)*
                        #seal_batch_arm
                        _ => ::core::result::Result::Ok(query),
                    }
                }
            };

            let to_row_impl = if struct_attrs.no_from_row {
                quote! {}
            } else if needs_key_manager {
                // A secret-bearing model's plain `bind_*` delegate to `*_encrypted` with no key and
                // panic.
                quote! {
                    impl #sql::ToRow for #name {
                        fn columns() -> &'static [&'static str] {
                            &[#(#all_col_names,)* #seal_column]
                        }

                        fn columns_without_pk() -> &'static [&'static str] {
                            &[#(#non_pk_col_names,)* #seal_column]
                        }

                        /// # Panics
                        ///
                        /// This model has `Secret` columns, which require a key manager
                        /// to encrypt. The plain `bind_to` panics; bind it through the
                        /// repository (which calls `bind_to_encrypted`) instead.
                        fn bind_to<'q, Q>(&'q self, query: Q) -> Q
                        where
                            Q: #sql::BindValues<'q>,
                        {
                            self.bind_to_encrypted(query, ::core::option::Option::None)
                                .expect(#secret_plain_panic)
                        }

                        /// # Panics
                        ///
                        /// Panics for the same reason as [`bind_to`](Self::bind_to).
                        fn bind_without_pk<'q, Q>(&'q self, query: Q) -> Q
                        where
                            Q: #sql::BindValues<'q>,
                        {
                            self.bind_without_pk_encrypted(query, ::core::option::Option::None)
                                .expect(#secret_plain_panic)
                        }

                        /// # Panics
                        ///
                        /// Panics for the same reason as [`bind_to`](Self::bind_to).
                        fn bind_key<'q, Q>(&'q self, query: Q) -> Q
                        where
                            Q: #sql::BindValues<'q>,
                        {
                            self.bind_key_encrypted(query, ::core::option::Option::None)
                                .expect(#secret_plain_panic)
                        }

                        /// # Panics
                        ///
                        /// Panics for the same reason as [`bind_to`](Self::bind_to).
                        fn bind_column_for_batch<'q, Q>(&'q self, query: Q, column_index: usize) -> Q
                        where
                            Q: #sql::BindValues<'q>,
                        {
                            self.bind_column_for_batch_encrypted(query, column_index, ::core::option::Option::None)
                                .expect(#secret_plain_panic)
                        }

                        #encrypted_bind_methods
                    }
                }
            } else {
                quote! {
                    impl #sql::ToRow for #name {
                        fn columns() -> &'static [&'static str] {
                            &[#(#all_col_names),*]
                        }

                        fn columns_without_pk() -> &'static [&'static str] {
                            &[#(#non_pk_col_names),*]
                        }

                        fn bind_to<'q, Q>(&'q self, query: Q) -> Q
                        where
                            Q: #sql::BindValues<'q>,
                        {
                            #(#bind_all_fields)*
                            query
                        }

                        fn bind_without_pk<'q, Q>(&'q self, query: Q) -> Q
                        where
                            Q: #sql::BindValues<'q>,
                        {
                            #(#bind_non_pk_fields)*
                            query
                        }

                        fn bind_key<'q, Q>(&'q self, query: Q) -> Q
                        where
                            Q: #sql::BindValues<'q>,
                        {
                            #(#bind_key_fields)*
                            query
                        }

                        fn bind_column_for_batch<'q, Q>(&'q self, query: Q, column_index: usize) -> Q
                        where
                            Q: #sql::BindValues<'q>,
                        {
                            match column_index {
                                #(#bind_batch_arms)*
                                _ => query,
                            }
                        }
                    }
                }
            };

            let model_query_methods = if struct_attrs.no_from_row {
                quote! {}
            } else {
                let id_ty = id_type;
                let has_to_row = !struct_attrs.no_from_row; // ToRow is generated alongside FromRow

                let insert_method = if !has_to_row {
                    quote! {}
                } else if cr_has_localized {
                    quote! {
                        /// Inserts this model and its translations atomically.
                        ///
                        /// Accepts both `&SqlPool` and `&mut SqlTransaction`.
                        pub async fn insert(&self, conn: impl Into<#sql::SqlConn<'_>>) -> #db::DatabaseResult<()> {
                            #sql::insert_translated_model(conn.into(), self, #table_name).await
                        }
                    }
                } else {
                    quote! {
                        /// Inserts this model into the database.
                        ///
                        /// Accepts both `&SqlPool` and `&mut SqlTransaction`.
                        pub async fn insert(&self, conn: impl Into<#sql::SqlConn<'_>>) -> #db::DatabaseResult<()> {
                            use #sql::ToRow;
                            let mut conn = conn.into();
                            // Schema-qualified: a bare name resolves through `search_path`, where a
                            // like-named table in an earlier schema intercepts the write
                            // (CVE-2018-1058).
                            let table = conn.schema().qualify(#table_name);
                            let columns = Self::columns();
                            let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("${}", i)).collect();
                            let sql = format!(
                                "INSERT INTO {} ({}) VALUES ({})",
                                table,
                                columns.join(", "),
                                placeholders.join(", "),
                            );
                            let query = #sql::sqlx::query(#sql::sqlx::AssertSqlSafe(sql.clone()));
                            let query = self.bind_to(query);
                            conn.execute_raw(query).await
                                .map(|_| ())
                                .map_err(|e| #db::DatabaseError::Query {
                                    message: e.to_string(),
                                    query: Some(sql),
                                })
                        }
                    }
                };

                // The update-builder has no model instance to read translations from, hence this
                // `upsert`.
                let upsert_method = if has_to_row && cr_has_localized {
                    quote! {
                        /// Inserts or updates this model and its translations atomically.
                        ///
                        /// On a primary-key conflict the row is updated and its companion
                        /// translation rows are rewritten (a locale dropped from the model
                        /// is removed). Accepts both `&SqlPool` and
                        /// `&mut SqlTransaction`.
                        pub async fn upsert(&self, conn: impl Into<#sql::SqlConn<'_>>) -> #db::DatabaseResult<()> {
                            #sql::upsert_translated_model(conn.into(), self, #table_name).await
                        }
                    }
                } else {
                    quote! {}
                };

                let find_translation_method = if has_to_row && cr_has_localized {
                    // Like `id_methods`: a composite-key model has no `find` to hand these ids to.
                    if id_fields.len() > 1 {
                        quote! {}
                    } else if let Some(id_ty) = id_type {
                        quote! {
                            /// Finds the ids of rows whose translatable `field` matches
                            /// `pattern` (SQL `ILIKE`, `%`/`_` wildcards) in `locale`.
                            /// Load each with [`find`](Self::find).
                            pub async fn find_ids_by_translation(
                                conn: impl Into<#sql::SqlConn<'_>>,
                                field: &str,
                                locale: &str,
                                pattern: &str,
                            ) -> #db::DatabaseResult<::std::vec::Vec<#id_ty>> {
                                #sql::find_translation_ids::<Self>(
                                    conn.into(), field, locale, pattern,
                                ).await
                            }
                        }
                    } else {
                        quote! {}
                    }
                } else {
                    quote! {}
                };

                // A composite key gets no `find`: half a key has no index prefix and returns
                // whichever tenant matched first. Fetch through the query surface with every key
                // column named.
                let id_methods = if id_fields.len() > 1 {
                    quote! {}
                } else if let Some(id_ty) = id_ty {
                    quote! {
                        /// Finds a model by its primary key.
                        ///
                        /// Accepts both `&SqlPool` and `&mut SqlTransaction`.
                        // One signature for every id type borrows a `String` id as `&String`.
                        #[allow(clippy::ptr_arg)]
                        pub async fn find(
                            conn: impl Into<#sql::SqlConn<'_>>,
                            id: &#id_ty,
                        ) -> #db::DatabaseResult<Option<Self>> {
                            #sql::SqlModelQuery::<Self>::find_by_id(conn, id)
                                .await
                        }

                        /// Returns an update builder for this model, targeting a specific ID.
                        ///
                        /// Accepts both `&SqlPool` and `&mut SqlTransaction`.
                        #[allow(clippy::ptr_arg)]
                        pub fn update<'c>(conn: impl Into<#sql::SqlConn<'c>>, id: &#id_ty) -> #sql::SqlUpdateBuilder<'c, Self> {
                            // By value, so the WHERE binds its native sqlx type rather than text.
                            #sql::SqlUpdateBuilder::<Self>::for_id(conn, id.clone())
                        }

                        /// Deletes a row by primary key. Returns the number of rows deleted (0 or 1).
                        ///
                        /// Accepts both `&SqlPool` and `&mut SqlTransaction`.
                        #[allow(clippy::ptr_arg)]
                        pub async fn delete(
                            conn: impl Into<#sql::SqlConn<'_>>,
                            id: &#id_ty,
                        ) -> #db::DatabaseResult<u64> {
                            #sql::SqlModelQuery::<Self>::delete_by_id(conn, id)
                                .await
                        }
                    }
                } else {
                    quote! {}
                };

                // Suppressed for a composite key: an unfiltered count or update would cross every
                // leading key value; the query surface reaches them with the leading key pinned.
                let whole_table_methods = if id_fields.len() > 1 {
                    quote! {}
                } else {
                    quote! {
                        /// Returns an update builder with no pre-applied ID filter.
                        ///
                        /// Use `.filter()` to specify which rows to update. This is useful
                        /// for updates that target rows by non-primary-key columns.
                        ///
                        /// Accepts both `&SqlPool` and `&mut SqlTransaction`.
                        pub fn update_where<'c>(conn: impl Into<#sql::SqlConn<'c>>) -> #sql::SqlUpdateBuilder<'c, Self> {
                            #sql::SqlUpdateBuilder::<Self>::new(conn)
                        }

                        /// Returns the total count of rows in this model's table.
                        ///
                        /// Accepts both `&SqlPool` and `&mut SqlTransaction`.
                        pub async fn count(conn: impl Into<#sql::SqlConn<'_>>) -> #db::DatabaseResult<u64> {
                            #sql::SqlModelQuery::<Self>::new(conn)
                                .count()
                                .await
                        }
                    }
                };

                quote! {
                    #id_methods

                    /// Returns a query builder for this model.
                    ///
                    /// Accepts both `&SqlPool` and `&mut SqlTransaction`.
                    pub fn query<'c>(conn: impl Into<#sql::SqlConn<'c>>) -> #sql::SqlModelQuery<'c, Self> {
                        #sql::SqlModelQuery::<Self>::new(conn)
                    }

                    #whole_table_methods

                    #insert_method

                    #upsert_method

                    #find_translation_method
                }
            };

            let model_query_impl = if struct_attrs.no_from_row {
                quote! {}
            } else {
                quote! {
                    impl #name {
                        #model_query_methods
                    }
                }
            };

            // Every key column in key order: UPDATE, DELETE and ON CONFLICT match the whole key.
            let cr_key_columns: Vec<String> = field_infos
                .iter()
                .filter(|f| f.attrs.is_id)
                .map(|f| {
                    f.attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string())
                })
                .collect();

            // Emitted only for a one-column key, and the bound on every fetch-by-id path. The
            // leading key column's field and type: the whole key, or a composite key's first part.
            let (id_ident, id_ty) = match (id_field, id_type) {
                (Some(i), Some(t)) => (i, t),
                _ => {
                    return Err(syn::Error::new_spanned(
                        &input.ident,
                        "YSQL models require a `#[model(id)]` primary key field.",
                    ));
                }
            };

            let composite_key = cr_key_columns.len() > 1;

            // Each declared index is checked against the model's columns (an outbox's framework
            // columns included), and its name becomes a constant, so it must be snake case.
            let outbox_columns: &[&str] = if struct_attrs.outbox {
                &[
                    "outbox_seal",
                    "outbox_set_aside_at",
                    "outbox_set_aside_reason",
                    "outbox_set_aside_seal",
                    "outbox_skip_until",
                ]
            } else {
                &[]
            };
            let cr_column_names: Vec<String> = field_infos
                .iter()
                .map(|f| {
                    f.attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string())
                })
                .chain(outbox_columns.iter().map(|column| (*column).to_owned()))
                .collect();
            let mut cr_index_defs = Vec::with_capacity(struct_attrs.indexes.len());
            let mut cr_index_consts = Vec::with_capacity(struct_attrs.indexes.len());
            let mut cr_index_adds: Vec<proc_macro2::TokenStream> = struct_attrs
                .checks
                .iter()
                .map(|(check_name, expr)| {
                    quote! {
                        __table.add_check_constraint(#db::migration::TableCheckConstraint {
                            name: #check_name.to_string(),
                            expression: #expr.to_string(),
                        });
                    }
                })
                .collect();
            for retired in &struct_attrs.retired.columns {
                if cr_column_names.contains(&retired.value()) {
                    return Err(syn::Error::new_spanned(
                        retired,
                        "a retired column is one the model no longer has; this one it still has",
                    ));
                }
            }
            for (role, when) in &struct_attrs.readers {
                cr_index_adds.push(quote! {
                    __table.add_reader(#role, #when);
                });
            }
            for role in &struct_attrs.writers {
                cr_index_adds.push(quote! { __table.add_writer(#role); });
            }
            if let Some(tablets) = &struct_attrs.tablets {
                // YugabyteDB splits into a number of tablets only by hash; `SPLIT INTO` cannot name
                // values.
                let leading = field_infos.iter().find(|f| f.attrs.is_id);
                // An undeclared leading part is hashed, as `KeyPart::parts` writes it.
                if !leading.is_some_and(|f| {
                    matches!(
                        f.attrs.shard,
                        None | Some(fundamental_core::model::sql::ShardKind::Hash)
                    )
                }) {
                    return Err(syn::Error::new_spanned(
                        tablets,
                        "`tablets` splits a table by hash, so its key leads with a \
                         `#[model(id, shard = hash)]` column; a range-split key is split at \
                         values instead",
                    ));
                }
                cr_index_adds.push(quote! { __table.set_tablets(#tablets); });
                if struct_attrs.colocated.is_none() {
                    cr_index_adds.push(quote! { __table.set_colocated(false); });
                }
            }
            if let Some(colocated) = &struct_attrs.colocated {
                cr_index_adds.push(quote! { __table.set_colocated(#colocated); });
            }
            for (lint, reason) in &struct_attrs.accepted {
                let variant = syn::Ident::new(&format!("{lint:?}"), reason.span());
                cr_index_adds.push(quote! {
                    __table.accept(#core::model::sql::Lint::#variant, #reason);
                });
            }
            let retired_columns = &struct_attrs.retired.columns;
            let retired_indexes = &struct_attrs.retired.indexes;
            let retired_checks = &struct_attrs.retired.checks;
            cr_index_adds.push(quote! {
                #( __table.retire_column(#retired_columns); )*
                #( __table.retire_index(#retired_indexes); )*
                #( __table.retire_check(#retired_checks); )*
            });
            let mut renamed_from: Vec<&String> = Vec::new();
            for field in &field_infos {
                if let Some(was) = &field.attrs.was {
                    let column = field
                        .attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| field.ident.to_string());
                    if cr_column_names.contains(was) {
                        return Err(syn::Error::new_spanned(
                            field.ident,
                            "`was` names the column's name before, which no other column may have",
                        ));
                    }
                    if renamed_from.contains(&was) {
                        return Err(syn::Error::new_spanned(
                            field.ident,
                            "another column already says it `was` this one; only one column can \
                             have been it",
                        ));
                    }
                    renamed_from.push(was);
                    cr_index_adds.push(quote! { __table.rename_column(#column, #was); });
                }
            }
            for index in &struct_attrs.indexes {
                let index_name = index.name.value();
                if index_name.is_empty()
                    || !index_name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                    || index_name.starts_with(|c: char| c.is_ascii_digit())
                {
                    return Err(syn::Error::new_spanned(
                        &index.name,
                        "an index name is the migration's snake_case name, and it becomes the \
                         model's constant",
                    ));
                }
                if index.columns.is_empty() {
                    return Err(syn::Error::new_spanned(
                        &index.name,
                        "an index needs at least one column: the leading one is what a query pins",
                    ));
                }
                for (written, column, _, _) in &index.columns {
                    if !cr_column_names.contains(column) {
                        return Err(syn::Error::new_spanned(
                            written,
                            format!(
                                "`{column}` is not a column of `{name}`; an index names the model's own columns"
                            ),
                        ));
                    }
                }
                for covered in &index.include {
                    if !cr_column_names.contains(&covered.value()) {
                        return Err(syn::Error::new_spanned(
                            covered,
                            format!(
                                "`{}` is not a column of `{name}`; an index carries the model's own columns",
                                covered.value()
                            ),
                        ));
                    }
                }
                let columns: Vec<String> = index
                    .columns
                    .iter()
                    .map(|(_, column, _, _)| column.clone())
                    .collect();
                let key_columns: Vec<proc_macro2::TokenStream> = index
                    .columns
                    .iter()
                    .map(|(_, column, split, nulls_first)| {
                        let split = split_tokens(&core, *split);
                        let nulls = match nulls_first {
                            None => quote! { ::core::option::Option::None },
                            Some(true) => quote! { ::core::option::Option::Some(#db::migration::Nulls::First) },
                            Some(false) => quote! { ::core::option::Option::Some(#db::migration::Nulls::Last) },
                        };
                        quote! { (#column, #split, #nulls) }
                    })
                    .collect();
                let unique = index.unique;
                let constraint = index.constraint;
                let condition = match &index.partial {
                    Some(partial) => quote! { ::core::option::Option::Some(#partial.to_string()) },
                    None => quote! { ::core::option::Option::None },
                };
                let include: Vec<String> = index.include.iter().map(syn::LitStr::value).collect();
                cr_index_adds.push(quote! {
                    __table.add_index(#db::migration::TableIndex {
                        name: #index_name.to_string(),
                        columns: ::std::vec![ #(#columns.to_string()),* ],
                        unique: #unique,
                        condition: #condition,
                        key: #db::migration::KeyPart::ordered_parts(&[ #(#key_columns),* ]),
                        include: ::std::vec![ #(#include.to_string()),* ],
                        constraint: #constraint,
                    });
                });
                let constant = format_ident!("{}", index_name.to_ascii_uppercase());
                let doc = format!(
                    "The `{index_name}` index of [`{name}`] — `({})`, led by `{}`.",
                    columns.join(", "),
                    columns[0]
                );
                let unique_def = unique.then(|| quote! { .unique() });
                let condition_def = index
                    .partial
                    .as_ref()
                    .map(|partial| quote! { .with_condition(#partial) });
                cr_index_defs.push(quote! {
                    #core::model::IndexDef::new(#index_name, &[#(#columns),*])
                        #unique_def #condition_def
                });
                cr_index_consts.push(quote! {
                    #[doc = #doc]
                    pub const #constant: #core::model::IndexKey<#name> =
                        #core::model::IndexKey::declared(#index_name, &[#(#columns),*]);
                });
            }
            let cr_index_impl = if cr_index_consts.is_empty() {
                quote! {}
            } else {
                quote! {
                    impl #name {
                        #(#cr_index_consts)*
                    }
                }
            };
            // Exactly one of the two markers, by the key's width.
            let cr_single_key_impl = if composite_key {
                // The filter and order of `cr_key_columns`, so the tuple and `KEY_COLUMNS` agree.
                let cr_key_value_tys: Vec<_> = field_infos
                    .iter()
                    .filter(|f| f.attrs.is_id)
                    .map(|f| f.ty)
                    .collect();
                let cr_key_value_idents: Vec<_> = field_infos
                    .iter()
                    .filter(|f| f.attrs.is_id)
                    .map(|f| f.ident)
                    .collect();
                quote! {
                    impl #core::model::CompositeKeyModel for #name {
                        type KeyValues = (#(#cr_key_value_tys,)*);

                        fn key_values(&self) -> Self::KeyValues {
                            (#(::core::clone::Clone::clone(&self.#cr_key_value_idents),)*)
                        }
                    }
                }
            } else {
                quote! { impl #core::model::SingleKeyModel for #name {} }
            };

            // A composite key's `Id` is a data-less marker and deliberately not `sqlx::Encode`, so
            // `find_by_id`, which requires it, does not compile.
            let cr_key_marker = format_ident!("{}Key", name);
            // Continuation lines start at column 0: indented four spaces, rustdoc reads a doctest.
            let cr_key_doc = format!(
                "The composite primary key of [`{name}`] — `({})`.\n\
                 \n\
                 A marker, not a value: it carries no data and is deliberately not \
                 `sqlx::Encode`, so `find_by_id` on this model is a compile error rather \
                 than a scan over a partial key. Fetch one through the query surface with \
                 every key column named.",
                cr_key_columns.join(", ")
            );
            let (cr_id_assoc, cr_key_marker_def) = if composite_key {
                (
                    quote! {
                        type Id = #cr_key_marker;
                        fn id(&self) -> &Self::Id { &#cr_key_marker }
                    },
                    quote! {
                        #[doc = #cr_key_doc]
                        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
                        pub struct #cr_key_marker;
                    },
                )
            } else {
                (
                    quote! {
                        type Id = #id_ty;
                        fn id(&self) -> &Self::Id { &self.#id_ident }
                    },
                    quote! {},
                )
            };

            // The model as its row gives it back: moments cut to the microsecond, unkept fields
            // default.
            let cr_into_stored = if struct_attrs.no_from_row {
                quote! {}
            } else {
                let kept: Vec<proc_macro2::TokenStream> = field_infos
                    .iter()
                    .filter_map(|f| {
                        let field = f.ident;
                        if f.attrs.skip {
                            Some(quote! { self.#field = ::core::default::Default::default(); })
                        } else if !FieldInfo::is_moment_type(f.ty) {
                            None
                        } else if FieldInfo::is_option_type(f.ty) {
                            Some(quote! {
                                self.#field = self.#field.map(#core::model::sql::stored_moment);
                            })
                        } else {
                            Some(quote! {
                                self.#field = #core::model::sql::stored_moment(self.#field);
                            })
                        }
                    })
                    .collect();
                if kept.is_empty() {
                    quote! {}
                } else {
                    quote! {
                        fn into_stored(mut self) -> Self {
                            #(#kept)*
                            self
                        }
                    }
                }
            };

            let cr_specs: Vec<ColumnSpec> = field_infos
                .iter()
                .filter_map(|f| f.column_spec(SchemaBackend::Postgres))
                .collect();
            let cr_pk: Vec<String> = cr_specs
                .iter()
                .filter(|s| s.primary_key)
                .map(|s| s.name.clone())
                .collect();
            let cr_overlay_adds = sync_overlay_schema_adds(
                &db,
                struct_attrs.syncable,
                &table_name,
                "TIMESTAMPTZ",
                "BIGINT",
                "BOOL",
                true,
            );
            // An outbox table's framework columns: the seal, and where a claim records a row it
            // could not relay.
            let cr_outbox_adds = if struct_attrs.outbox {
                let framework_column =
                    |column: proc_macro2::TokenStream, data_type: &str, nullable: bool| {
                        quote! {
                            __table.add_column(#db::migration::TableColumn {
                                name: #column.to_string(),
                                data_type: #data_type.to_string(),
                                nullable: #nullable,
                                default: ::core::option::Option::None,
                                primary_key: false,
                                unique: false,
                            });
                        }
                    };
                let seal = framework_column(
                    quote! { #core::outbox::OutboxColumns::SEAL },
                    "BYTEA",
                    false,
                );
                let set_aside_at = framework_column(
                    quote! { #core::outbox::OutboxColumns::SET_ASIDE_AT },
                    "TIMESTAMPTZ",
                    true,
                );
                let set_aside_reason = framework_column(
                    quote! { #core::outbox::OutboxColumns::SET_ASIDE_REASON },
                    "TEXT",
                    true,
                );
                let set_aside_seal = framework_column(
                    quote! { #core::outbox::OutboxColumns::SET_ASIDE_SEAL },
                    "BYTEA",
                    true,
                );
                let skip_until = framework_column(
                    quote! { #core::outbox::OutboxColumns::SKIP_UNTIL },
                    "TIMESTAMPTZ",
                    true,
                );
                quote! { #seal #set_aside_at #set_aside_reason #set_aside_seal #skip_until }
            } else {
                quote! {}
            };
            let cr_model_schema = generate_model_schema_impl(
                &db,
                &core,
                name,
                &quote! { #core::model::SqlDb },
                &quote! { #db::migration::MigrationDatabase::SqlDb },
                &table_name,
                &cr_specs,
                soft_delete,
                "TIMESTAMPTZ",
                &cr_pk,
                None,
                None,
                quote! { #cr_overlay_adds #cr_outbox_adds #(#cr_index_adds)* },
                translation_schema_item(
                    &db,
                    quote! { #core::model::SqlDb },
                    &quote! { #db::migration::MigrationDatabase::SqlDb },
                    cr_has_localized,
                ),
            );

            let effect_model = if framework_table {
                quote! {}
            } else {
                quote! { impl #db::EffectModel for #name {} }
            };
            quote! {
                impl #core::model::BoundTo<#core::model::SqlDb> for #name {}

                #effect_model

                #cr_model_schema

                #cr_key_marker_def

                impl #core::model::SqlModel for #name {
                    #cr_id_assoc

                    #cr_into_stored

                    const TABLE: &'static str = #table_name;
                    const PRIMARY_KEY: &'static str = #primary_key;
                    const KEY_COLUMNS: &'static [&'static str] = &[#(#cr_key_columns),*];
                    const INDEXES: &'static [#core::model::IndexDef] = &[#(#cr_index_defs),*];
                    const DOMAIN: &'static str = #domain_expr;
                    const SOFT_DELETE: bool = #soft_delete;

                    #cr_translation_methods
                }

                #cr_single_key_impl

                #cr_index_impl

                #from_row_impl

                #to_row_impl

                impl #name {
                    #(#column_constants)*

                    /// The generated `CREATE TABLE` statement for this model's table.
                    ///
                    /// Best-effort: id newtypes map to `TEXT` (see
                    /// [`rust_type_to_postgres`]). Provide an explicit migration for
                    /// non-text id types.
                    pub fn create_table_sql() -> &'static str { #cr_main_ddl }
                }

                #model_query_impl
            }
        }
        Some(DatabaseBinding::CqlDb) => {
            let table_name = row_table_name.clone();

            let domain_expr = connection_domain_expr(&struct_attrs, &core);

            if partition_key_fields.is_empty() {
                return Err(syn::Error::new_spanned(
                    input,
                    "YCQL models (`#[model(cql)]`) require at least one partition key field.\n\n\
                     Add `#[model(partition_key)]` to a field:\n\
                     ```\n\
                     #[model(partition_key)]\n\
                     tenant_id: TenantId,\n\
                     ```\n\n\
                     The partition key determines how data is distributed across nodes.",
                ));
            }

            // The YCQL encrypted codec assumes a non-optional envelope, so a nullable secret is
            // refused.
            if let Some(f) = field_infos
                .iter()
                .find(|f| !f.attrs.skip && FieldInfo::optional_secret_inner(f.ty).is_some())
            {
                return Err(syn::Error::new_spanned(
                    f.ty,
                    "A nullable secret column (`Option<Secret*>`) is not yet supported on a \
                     YCQL model.\n\n\
                     Use a non-optional secret (`SecretString`/`SecretBytes`/`SecretJson<T>`) — \
                     a CqlDb cache row that may be absent is modelled by deleting the partition, \
                     not by a NULL secret column. `Option<Secret*>` is available on `#[model(sql)]`.",
                ));
            }

            let partition_key_names: Vec<String> = partition_key_fields
                .iter()
                .map(|f| {
                    f.attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string())
                })
                .collect();

            let clustering_key_names: Vec<String> = clustering_key_fields
                .iter()
                .map(|f| {
                    f.attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string())
                })
                .collect();

            let partition_key_type = if partition_key_fields.len() == 1 {
                let ty = partition_key_fields[0].ty;
                quote! { #ty }
            } else {
                let types: Vec<_> = partition_key_fields.iter().map(|f| f.ty).collect();
                quote! { (#(#types),*) }
            };

            let clustering_key_type = if clustering_key_fields.is_empty() {
                quote! { () }
            } else if clustering_key_fields.len() == 1 {
                let ty = clustering_key_fields[0].ty;
                quote! { #ty }
            } else {
                let types: Vec<_> = clustering_key_fields.iter().map(|f| f.ty).collect();
                quote! { (#(#types),*) }
            };

            let all_column_names: Vec<String> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip)
                .map(|f| {
                    f.attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string())
                })
                .collect();

            let all_fields: Vec<&FieldInfo> =
                field_infos.iter().filter(|f| !f.attrs.skip).collect();
            let value_fields: Vec<&FieldInfo> = field_infos
                .iter()
                .filter(|f| {
                    !f.attrs.skip && !f.attrs.partition_key && f.attrs.clustering_key.is_none()
                })
                .collect();

            // A `Secret` slot is its `String` envelope; `Localized` source values are TEXT too.
            let wire_type = |f: &FieldInfo| -> proc_macro2::TokenStream {
                if FieldInfo::is_secret_type(f.ty) || FieldInfo::is_localized_type(f.ty) {
                    quote! { String }
                } else {
                    let ty = f.ty;
                    quote! { #ty }
                }
            };
            // As on YSQL, a secret-bearing model gets panicking plain codecs.
            let sc_has_secret = !secret_fields.is_empty();

            // Plain codec: a secret-bearing model never emits this for a `Secret` field.
            let wire_expr = |f: &FieldInfo| -> proc_macro2::TokenStream {
                let ident = f.ident;
                if FieldInfo::is_localized_type(f.ty) {
                    quote! { ::std::string::ToString::to_string(#core::i18n::Localized::source(&self.#ident)) }
                } else {
                    let ty = f.ty;
                    let ty_str = quote!(#ty).to_string();
                    // Primitives are Copy.
                    if matches!(
                        ty_str.as_str(),
                        "i64" | "i32" | "i16" | "i8" | "f64" | "f32" | "bool"
                    ) {
                        quote! { self.#ident }
                    } else {
                        quote! { self.#ident.clone() }
                    }
                }
            };

            // A `Secret` column yields its envelope encrypted through `enc`; the `?` propagates
            // from the enclosing `*_encrypted` method.
            let wire_expr_encrypted = |f: &FieldInfo| -> proc_macro2::TokenStream {
                if FieldInfo::is_secret_type(f.ty) {
                    let ident = f.ident;
                    let col = f
                        .attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string());
                    let key_id = f.encryption_key_id();
                    let encrypt = if FieldInfo::is_secret_bytes_type(f.ty) {
                        quote! { __km.encrypt_bytes(self.#ident.expose(), #key_id, &#enc::RowBinding::new(#row_table_name, #col, &__row_parts)) }
                    } else if FieldInfo::is_secret_json_type(f.ty) {
                        quote! { __km.encrypt_json_to_string(self.#ident.expose(), #key_id, &#enc::RowBinding::new(#row_table_name, #col, &__row_parts)) }
                    } else {
                        quote! { __km.encrypt_string(self.#ident.expose(), #key_id, &#enc::RowBinding::new(#row_table_name, #col, &__row_parts)) }
                    };
                    quote! {
                        {
                            let __km = enc.ok_or_else(|| #cql::CqlError::Config(
                                format!("model column '{}' is an encrypted Secret but no key manager was provided", #col)))?;
                            let __ct = #encrypt
                                .map_err(|e| #cql::CqlError::Config(
                                    format!("encrypt secret column '{}' failed: {}", #col, e)))?;
                            #enc::encode_envelope(&__ct)
                        }
                    }
                } else {
                    wire_expr(f)
                }
            };

            // The trailing comma makes a 1-tuple: only tuples implement `SerializeRow`.
            let insert_value_types: Vec<_> = all_fields.iter().map(|&f| wire_type(f)).collect();
            let insert_values_type = quote! { (#(#insert_value_types,)*) };

            let update_value_types: Vec<_> = value_fields
                .iter()
                .chain(partition_key_fields.iter())
                .chain(clustering_key_fields.iter())
                .map(|&f| wire_type(f))
                .collect();
            let update_values_type = quote! { (#(#update_value_types,)*) };

            // Keys are never encrypted.
            let key_value_types: Vec<_> = partition_key_fields
                .iter()
                .chain(clustering_key_fields.iter())
                .map(|&f| wire_type(f))
                .collect();
            let key_values_type = quote! { (#(#key_value_types,)*) };

            let to_values_fields: Vec<_> = all_fields.iter().map(|&f| wire_expr(f)).collect();
            let to_update_fields: Vec<_> = value_fields
                .iter()
                .chain(partition_key_fields.iter())
                .chain(clustering_key_fields.iter())
                .map(|&f| wire_expr(f))
                .collect();
            let to_key_fields: Vec<_> = partition_key_fields
                .iter()
                .chain(clustering_key_fields.iter())
                .map(|&f| wire_expr(f))
                .collect();

            let to_values_fields_enc: Vec<_> =
                all_fields.iter().map(|&f| wire_expr_encrypted(f)).collect();
            let to_update_fields_enc: Vec<_> = value_fields
                .iter()
                .chain(partition_key_fields.iter())
                .chain(clustering_key_fields.iter())
                .map(|&f| wire_expr_encrypted(f))
                .collect();
            // At the CQL wire's millisecond precision.
            let cql_bound_parts_from_self: Vec<_> = bound_fields
                .iter()
                .map(|f| {
                    let ident = f.ident;
                    {
                    let name = f.attrs.column.clone().unwrap_or_else(|| f.ident.to_string());
                    quote! { (#name, #core::model::RowBound::row_part(&self.#ident, #core::model::TimestampPrecision::Milliseconds)) }
                    }
                })
                .collect();

            let from_row_extractions: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip)
                .enumerate()
                .map(|(idx, f)| {
                    let field_ident = f.ident;
                    let col_name = f
                        .attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string());
                    FieldInfo::cql_from_row_field_extraction(field_ident, f.ty, idx, &col_name)
                })
                .collect();

            // Non-secret columns first, the row binding from them, then each `Secret` decrypted
            // under it; indices are positions among the persisted columns, as `columns()` selects
            // them.
            let cql_local_of =
                |f: &FieldInfo| Ident::new(&format!("__col_{}", f.ident), f.ident.span());
            let cql_persisted: Vec<(usize, &FieldInfo)> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip)
                .enumerate()
                .collect();
            let cql_plain_locals: Vec<proc_macro2::TokenStream> = cql_persisted
                .iter()
                .filter(|(_, f)| !FieldInfo::is_secret_type(f.ty))
                .map(|(idx, f)| {
                    let local = cql_local_of(f);
                    let col_name = f
                        .attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string());
                    let value = FieldInfo::cql_from_row_value_extraction(f.ty, *idx, &col_name);
                    quote! { let #local = #value; }
                })
                .collect();
            let cql_bound_parts_from_locals: Vec<proc_macro2::TokenStream> = bound_fields
                .iter()
                .map(|f| {
                    let local = cql_local_of(f);
                    {
                    let name = f.attrs.column.clone().unwrap_or_else(|| f.ident.to_string());
                    quote! { (#name, #core::model::RowBound::row_part(&#local, #core::model::TimestampPrecision::Milliseconds)) }
                    }
                })
                .collect();
            let cql_secret_locals: Vec<proc_macro2::TokenStream> = cql_persisted
                .iter()
                .filter(|(_, f)| FieldInfo::is_secret_type(f.ty))
                .map(|(idx, f)| {
                    let local = cql_local_of(f);
                    let col_name = f.attrs.column.clone().unwrap_or_else(|| f.ident.to_string());
                    let binding = quote! { &#enc::RowBinding::new(#row_table_name, #col_name, &__row_parts) };
                    let decrypt = if FieldInfo::is_secret_bytes_type(f.ty) {
                        quote! {
                            let __es: #enc::EncryptedBytes = #enc::decode_envelope(__s)
                                .map_err(|e| #cql::CqlError::RowConversion(
                                    format!("decode secret '{}' failed: {}", #col_name, e)))?;
                            let __pt = __km.decrypt_bytes(&__es, #binding)
                                .map_err(|e| #cql::CqlError::RowConversion(
                                    format!("decrypt secret '{}' failed: {}", #col_name, e)))?;
                        }
                    } else if let Some(inner) = FieldInfo::secret_json_inner(f.ty) {
                        quote! {
                            let __es: #enc::EncryptedString = #enc::decode_envelope(__s)
                                .map_err(|e| #cql::CqlError::RowConversion(
                                    format!("decode secret '{}' failed: {}", #col_name, e)))?;
                            let __pt = __km.decrypt_string_to_json::<#inner>(&__es, #binding)
                                .map_err(|e| #cql::CqlError::RowConversion(
                                    format!("decrypt secret '{}' failed: {}", #col_name, e)))?;
                        }
                    } else {
                        quote! {
                            let __es: #enc::EncryptedString = #enc::decode_envelope(__s)
                                .map_err(|e| #cql::CqlError::RowConversion(
                                    format!("decode secret '{}' failed: {}", #col_name, e)))?;
                            let __pt = __km.decrypt_string(&__es, #binding)
                                .map_err(|e| #cql::CqlError::RowConversion(
                                    format!("decrypt secret '{}' failed: {}", #col_name, e)))?;
                        }
                    };
                    quote! {
                        let #local = {
                            let __km = enc.ok_or_else(|| #cql::CqlError::Config(
                                format!("secret column '{}' needs a key manager to decrypt", #col_name)))?;
                            let __v = cols.get(#idx)
                                .ok_or_else(|| #cql::CqlError::RowConversion(
                                    format!("column {} (index {}) missing from row", #col_name, #idx)))?
                                .as_ref()
                                .ok_or_else(|| #cql::CqlError::RowConversion(
                                    format!("column {} (index {}) is null", #col_name, #idx)))?;
                            let __s = __v.as_text().ok_or_else(|| #cql::CqlError::RowConversion(
                                format!("column {} (index {}): expected text envelope", #col_name, #idx)))?;
                            #decrypt
                            #core::secrets::Secret::new(__pt)
                        };
                    }
                })
                .collect();
            let cql_fields_from_locals: Vec<proc_macro2::TokenStream> = cql_persisted
                .iter()
                .map(|(_, f)| {
                    let field_ident = f.ident;
                    let local = cql_local_of(f);
                    quote! { #field_ident: #local }
                })
                .collect();

            let from_row_skipped: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| f.attrs.skip)
                .map(|f| {
                    let field_ident = f.ident;
                    quote! { #field_ident: ::core::default::Default::default() }
                })
                .collect();

            let sc_secret_plain_panic = format!(
                "{type_name} has encrypted Secret columns; persist it through the \
                 key-manager-backed repository, not the plain codec"
            );

            // Secret-bearing models only; others inherit the trait-default `*_encrypted`.
            let clustering_key_orders: Vec<proc_macro2::TokenStream> = clustering_key_fields
                .iter()
                .map(
                    |f| match f.attrs.clustering_key.unwrap_or(ClusteringOrder::Asc) {
                        ClusteringOrder::Asc => quote! { #core::model::cql::ClusteringOrder::Asc },
                        ClusteringOrder::Desc => {
                            quote! { #core::model::cql::ClusteringOrder::Desc }
                        }
                    },
                )
                .collect();
            let cql_to_row_impl = if sc_has_secret {
                quote! {
                    impl #cql::ToRow for #name {
                        fn columns() -> &'static [&'static str] { &[#(#all_column_names),*] }
                        fn partition_key_columns() -> &'static [&'static str] { &[#(#partition_key_names),*] }
                        fn clustering_key_columns() -> &'static [&'static str] { &[#(#clustering_key_names),*] }
                        fn clustering_key_orders() -> &'static [#core::model::cql::ClusteringOrder] { &[#(#clustering_key_orders),*] }

                        type InsertValues = #insert_values_type;
                        type UpdateValues = #update_values_type;
                        type KeyValues = #key_values_type;

                        /// # Panics
                        ///
                        /// This model has `Secret` columns, which require a key manager
                        /// to encrypt. The plain `to_values` panics; persist it through
                        /// the repository (which calls `to_values_encrypted`) instead.
                        fn to_values(&self) -> Self::InsertValues {
                            self.to_values_encrypted(::core::option::Option::None)
                                .expect(#sc_secret_plain_panic)
                        }

                        /// # Panics
                        ///
                        /// Panics for the same reason as [`to_values`](Self::to_values).
                        fn to_update_values(&self) -> Self::UpdateValues {
                            self.to_update_values_encrypted(::core::option::Option::None)
                                .expect(#sc_secret_plain_panic)
                        }

                        fn to_key_values(&self) -> Self::KeyValues {
                            (#(#to_key_fields,)*)
                        }

                        fn to_values_encrypted(
                            &self,
                            enc: ::core::option::Option<&#enc::KeyManager>,
                        ) -> #cql::CqlResult<Self::InsertValues> {
                            let __row_parts: ::std::vec::Vec<(&'static str, ::core::option::Option<::std::vec::Vec<u8>>)> =
                                ::std::vec![#(#cql_bound_parts_from_self),*];
                            ::core::result::Result::Ok((#(#to_values_fields_enc,)*))
                        }

                        fn to_update_values_encrypted(
                            &self,
                            enc: ::core::option::Option<&#enc::KeyManager>,
                        ) -> #cql::CqlResult<Self::UpdateValues> {
                            let __row_parts: ::std::vec::Vec<(&'static str, ::core::option::Option<::std::vec::Vec<u8>>)> =
                                ::std::vec![#(#cql_bound_parts_from_self),*];
                            ::core::result::Result::Ok((#(#to_update_fields_enc,)*))
                        }
                    }
                }
            } else {
                quote! {
                    impl #cql::ToRow for #name {
                        fn columns() -> &'static [&'static str] { &[#(#all_column_names),*] }
                        fn partition_key_columns() -> &'static [&'static str] { &[#(#partition_key_names),*] }
                        fn clustering_key_columns() -> &'static [&'static str] { &[#(#clustering_key_names),*] }
                        fn clustering_key_orders() -> &'static [#core::model::cql::ClusteringOrder] { &[#(#clustering_key_orders),*] }

                        type InsertValues = #insert_values_type;
                        type UpdateValues = #update_values_type;
                        type KeyValues = #key_values_type;

                        fn to_values(&self) -> Self::InsertValues { (#(#to_values_fields,)*) }
                        fn to_update_values(&self) -> Self::UpdateValues { (#(#to_update_fields,)*) }
                        fn to_key_values(&self) -> Self::KeyValues { (#(#to_key_fields,)*) }
                    }
                }
            };

            let cql_from_row_impl = if sc_has_secret {
                quote! {
                    impl #cql::FromRow for #name {
                        /// This model has `Secret` columns, which need a key manager to decrypt,
                        /// so a row holding a value is an error here; read it through the
                        /// repository, which calls `from_row_encrypted`.
                        fn from_row(row: #cql::CqlRow) -> #cql::CqlResult<Self> {
                            Self::from_row_encrypted(row, ::core::option::Option::None)
                        }

                        fn from_row_encrypted(
                            row: #cql::CqlRow,
                            enc: ::core::option::Option<&#enc::KeyManager>,
                        ) -> #cql::CqlResult<Self> {
                            let cols = row.columns;
                            #(#cql_plain_locals)*
                            let __row_parts: ::std::vec::Vec<(&'static str, ::core::option::Option<::std::vec::Vec<u8>>)> =
                                ::std::vec![#(#cql_bound_parts_from_locals),*];
                            #(#cql_secret_locals)*
                            Ok(Self {
                                #(#cql_fields_from_locals,)*
                                #(#from_row_skipped,)*
                            })
                        }
                    }
                }
            } else {
                quote! {
                    impl #cql::FromRow for #name {
                        fn from_row(row: #cql::CqlRow) -> #cql::CqlResult<Self> {
                            let cols = row.columns;
                            Ok(Self {
                                #(#from_row_extractions,)*
                                #(#from_row_skipped,)*
                            })
                        }
                    }
                }
            };

            let cql_db_inner = quote! {
                #cql_to_row_impl

                #cql_from_row_impl
            };

            let cql_db_impl = cql_db_inner;

            // A YCQL model has no single id, so its companion table is keyed by its key as text.
            let sc_localized_fields: Vec<&FieldInfo> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && FieldInfo::is_localized_type(f.ty))
                .collect();
            let sc_has_localized = !sc_localized_fields.is_empty();
            let sc_localized_columns: Vec<String> = sc_localized_fields
                .iter()
                .map(|f| {
                    f.attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string())
                })
                .collect();
            let sc_translation_table = format!("{table_name}_i18n");
            let soft_delete = struct_attrs.soft_delete;
            let sc_clustering_pairs: Vec<(String, ClusteringOrder)> = clustering_key_fields
                .iter()
                .zip(clustering_key_names.iter())
                .map(|(f, col)| {
                    (
                        col.clone(),
                        f.attrs.clustering_key.unwrap_or(ClusteringOrder::Asc),
                    )
                })
                .collect();
            let sc_main_ddl = FieldInfo::generate_cql_create_table_sql(
                &table_name,
                &field_infos,
                &partition_key_names,
                &sc_clustering_pairs,
                soft_delete,
                struct_attrs.syncable,
            );
            let sc_translation_table_sql_method = if sc_has_localized {
                quote! {
                    /// `CREATE TABLE` for the companion translations table, written from
                    /// its one definition (`ModelSchema::translation_schema`).
                    pub fn translation_table_sql() -> &'static str {
                        static SQL: ::std::sync::LazyLock<::std::string::String> =
                            ::std::sync::LazyLock::new(|| {
                                <#name as #db::migration::ModelSchema<#core::model::CqlDb>>::translation_schema()
                                    .map(|companion| #db::migration::render::create_cql_table(&companion))
                                    .unwrap_or_default()
                            });
                        SQL.as_str()
                    }
                }
            } else {
                quote! {}
            };
            let sc_translation_methods = if sc_has_localized {
                let cols = &sc_localized_columns;
                let row_methods = translation_row_methods(&core, &sc_localized_fields, cols);
                quote! {
                    fn translatable_columns() -> &'static [&'static str] { &[#(#cols),*] }
                    fn translation_table() -> ::core::option::Option<&'static str> {
                        ::core::option::Option::Some(#sc_translation_table)
                    }
                    #row_methods
                }
            } else {
                quote! {}
            };

            // CQL has no column nullability, so every column is nullable, as `system_schema`
            // reports.
            let sc_specs: Vec<ColumnSpec> = field_infos
                .iter()
                .filter_map(|f| f.column_spec(SchemaBackend::Cql))
                .map(|mut spec| {
                    spec.nullable = true;
                    spec
                })
                .collect();
            let sc_pk: Vec<String> = partition_key_names
                .iter()
                .chain(clustering_key_names.iter())
                .cloned()
                .collect();
            let sc_overlay_adds = sync_overlay_schema_adds(
                &db,
                struct_attrs.syncable,
                &table_name,
                "timestamp",
                "bigint",
                "boolean",
                false,
            );
            let sc_model_schema = generate_model_schema_impl(
                &db,
                &core,
                name,
                &quote! { #core::model::CqlDb },
                &quote! { #db::migration::MigrationDatabase::CqlDb },
                &table_name,
                &sc_specs,
                soft_delete,
                "timestamp",
                &sc_pk,
                Some(&partition_key_names),
                Some(&sc_clustering_pairs),
                sc_overlay_adds,
                translation_schema_item(
                    &db,
                    quote! { #core::model::CqlDb },
                    &quote! { #db::migration::MigrationDatabase::CqlDb },
                    sc_has_localized,
                ),
            );

            // Exactly one of the two markers, by the partition's width; `SinglePartitionModel`
            // bounds `CqlQuery::partition_key`.
            let sc_partition_marker = if partition_key_fields.len() > 1 {
                // The filter and order of `partition_key_names`, so the tuple and
                // `partition_key_columns()` agree.
                let idents: Vec<_> = partition_key_fields.iter().map(|f| f.ident).collect();
                quote! {
                    impl #core::model::CompositePartitionModel for #name {
                        fn partition_key_values(&self) -> Self::PartitionKey {
                            (#(::core::clone::Clone::clone(&self.#idents),)*)
                        }
                    }
                }
            } else {
                quote! { impl #core::model::SinglePartitionModel for #name {} }
            };

            quote! {
                impl #core::model::BoundTo<#core::model::CqlDb> for #name {}

                #sc_model_schema

                impl #core::model::CqlModel for #name {
                    const TABLE: &'static str = #table_name;
                    const DOMAIN: &'static str = #domain_expr;
                    const SOFT_DELETE: bool = #soft_delete;
                    type PartitionKey = #partition_key_type;
                    type ClusteringKey = #clustering_key_type;

                    #sc_translation_methods
                }

                #sc_partition_marker

                #cql_db_impl

                impl #name {
                    #(#column_constants)*

                    /// The generated `CREATE TABLE` for this model's table.
                    pub fn create_table_sql() -> &'static str { #sc_main_ddl }

                    #sc_translation_table_sql_method
                }
            }
        }
        None => {
            quote! {}
        }
    };

    let offline_impl = if struct_attrs.backends.offline {
        let table_name = struct_attrs
            .offline_table
            .unwrap_or_else(|| FieldInfo::to_snake_case(&type_name));

        let preserve_data = struct_attrs.offline_preserve;

        let mut column_info: Vec<(String, String, bool)> = Vec::new();
        for field in &field_infos {
            if field.attrs.skip {
                continue;
            }
            let col_name = field
                .attrs
                .column
                .clone()
                .unwrap_or_else(|| field.ident.to_string());
            let col_type = FieldInfo::rust_type_to_sqlite(field.ty);
            let nullable = FieldInfo::is_option_type(field.ty);
            column_info.push((col_name, col_type.to_string(), nullable));
        }

        let localized_fields: Vec<&FieldInfo> = field_infos
            .iter()
            .filter(|f| !f.attrs.skip && FieldInfo::is_localized_type(f.ty))
            .collect();
        let localized_columns: Vec<String> = localized_fields
            .iter()
            .map(|f| {
                f.attrs
                    .column
                    .clone()
                    .unwrap_or_else(|| f.ident.to_string())
            })
            .collect();
        let translation_table_name = format!("{table_name}_i18n");

        // A marker per translatable column, so a field becoming `Localized` still recreates the
        // table.
        let mut hash_input = column_info.clone();
        for col in &localized_columns {
            hash_input.push((format!("{col}__i18n"), "TRANSLATABLE".to_string(), false));
        }
        if !localized_columns.is_empty() {
            // The companion is keyed by the record's key column, so a cache of the old shape is
            // rebuilt.
            hash_input.push(("__i18n_key".to_string(), "RECORD_KEY".to_string(), false));
        }
        if struct_attrs.soft_delete {
            hash_input.push(("deleted_at".to_string(), "TEXT".to_string(), true));
        }
        let schema_hash = FieldInfo::compute_schema_hash(&table_name, &hash_input);

        let core_path = crate::derives::core_path();
        let (translation_model_methods, translation_record_methods) = if localized_fields.is_empty()
        {
            (quote! {}, quote! {})
        } else {
            let cols = &localized_columns;
            let row_methods = translation_row_methods(&core_path, &localized_fields, cols);
            (
                quote! {
                    fn translatable_columns() -> &'static [&'static str] {
                        &[#(#cols),*]
                    }
                    fn translation_table() -> ::core::option::Option<&'static str> {
                        ::core::option::Option::Some(#translation_table_name)
                    }
                    fn translation_table_sql() -> ::core::option::Option<&'static str> {
                        static SQL: ::std::sync::LazyLock<::std::option::Option<::std::string::String>> =
                            ::std::sync::LazyLock::new(|| {
                                <#name as #db::migration::ModelSchema<#core::model::Offline>>::translation_schema()
                                    .map(|companion| #db::migration::render::create_sqlite_table(&companion))
                            });
                        SQL.as_deref()
                    }
                },
                quote! {
                    #row_methods
                },
            )
        };

        let column_names: Vec<&str> = column_info.iter().map(|(n, _, _)| n.as_str()).collect();

        let col_def_names: Vec<&str> = column_info.iter().map(|(n, _, _)| n.as_str()).collect();
        let col_def_types: Vec<&str> = column_info.iter().map(|(_, t, _)| t.as_str()).collect();
        let col_def_nullables: Vec<bool> = column_info.iter().map(|(_, _, n)| *n).collect();

        let soft_delete = struct_attrs.soft_delete;
        let create_table_sql =
            FieldInfo::generate_create_table_sql(&table_name, &field_infos, soft_delete);

        let offline = crate::derives::offline_path();
        let pk_column = field_infos
            .iter()
            .find(|f| f.attrs.is_id)
            .map(|f| {
                f.attrs
                    .column
                    .clone()
                    .unwrap_or_else(|| f.ident.to_string())
            })
            .unwrap_or_else(|| "id".to_string());
        let to_value_exprs: Vec<_> = field_infos
            .iter()
            .filter(|f| !f.attrs.skip)
            .map(|f| f.offline_to_value())
            .collect();
        let from_row_extractions: Vec<_> = field_infos
            .iter()
            .filter(|f| !f.attrs.skip)
            .map(|f| f.offline_from_row_extraction())
            .collect();
        let from_row_skipped: Vec<proc_macro2::TokenStream> = field_infos
            .iter()
            .filter(|f| f.attrs.skip)
            .map(|f| {
                let field_ident = f.ident;
                quote! { #field_ident: ::core::default::Default::default() }
            })
            .collect();

        let off_specs: Vec<ColumnSpec> = field_infos
            .iter()
            .filter_map(|f| f.column_spec(SchemaBackend::Sqlite))
            .collect();
        let off_pk: Vec<String> = off_specs
            .iter()
            .filter(|s| s.primary_key)
            .map(|s| s.name.clone())
            .collect();
        // The syncable overlay comes from `mirror_table_schema`, not here.
        let off_model_schema = generate_model_schema_impl(
            &db,
            &core,
            name,
            &quote! { #core::model::Offline },
            &quote! { #db::migration::MigrationDatabase::OfflineDb },
            &table_name,
            &off_specs,
            soft_delete,
            "TEXT",
            &off_pk,
            None,
            None,
            quote! {},
            translation_schema_item(
                &db,
                quote! { #core::model::Offline },
                &quote! { #db::migration::MigrationDatabase::OfflineDb },
                !localized_fields.is_empty(),
            ),
        );

        // Sync mirror columns: the offline columns minus `skip` and `no_sync`; a `Secret` syncs as
        // its ciphertext.
        let sync_impl = if struct_attrs.syncable {
            let sync = crate::derives::sync_path();
            let data_columns: Vec<proc_macro2::TokenStream> = field_infos
                .iter()
                .filter(|f| !f.attrs.skip && !f.attrs.no_sync)
                .map(|f| {
                    let col = f
                        .attrs
                        .column
                        .clone()
                        .unwrap_or_else(|| f.ident.to_string());
                    // A `Secret` is its TEXT envelope on the wire and in the mirror.
                    let storage = if FieldInfo::is_encrypted_column(f.ty) {
                        "TEXT"
                    } else {
                        FieldInfo::rust_type_to_sqlite(f.ty)
                    };
                    let nullable = FieldInfo::is_option_type(f.ty);
                    quote! {
                        #sync::SyncColumn {
                            name: #col,
                            storage_type: #storage,
                            nullable: #nullable,
                            role: #sync::SyncColumnRole::Data,
                        }
                    }
                })
                .collect();
            quote! {
                impl #sync::Syncable for #name {
                    const STRATEGY: #core::model::offline::ConflictStrategy =
                        <Self as #core::model::OfflineModel>::CONFLICT_RESOLUTION;
                    const MIRROR_TABLE: &'static str = #table_name;

                    fn sync_columns() -> &'static [#sync::SyncColumn] {
                        &[
                            #(#data_columns,)*
                            #sync::SyncColumn { name: "updated_at", storage_type: "TEXT", nullable: false, role: #sync::SyncColumnRole::UpdatedAt },
                            #sync::SyncColumn { name: "version", storage_type: "INTEGER", nullable: false, role: #sync::SyncColumnRole::Version },
                            #sync::SyncColumn { name: "_tombstone", storage_type: "INTEGER", nullable: false, role: #sync::SyncColumnRole::Tombstone },
                        ]
                    }

                    fn primary_key() -> &'static [&'static str] {
                        // Keyed as the server table is, so the mirror can place the key columns it
                        // is sent.
                        &[#(#syncable_key_columns),*]
                    }
                }
            }
        } else {
            quote! {}
        };

        quote! {
            impl #core::model::BoundTo<#core::model::Offline> for #name {}

            #off_model_schema

            #sync_impl

            impl #core::model::OfflineModel for #name {
                const SQLITE_TABLE: &'static str = #table_name;
                const SCHEMA_VERSION: u64 = #schema_hash;
                const PRESERVE_DATA: bool = #preserve_data;
                const SOFT_DELETE: bool = #soft_delete;

                fn create_table_sql() -> &'static str {
                    #create_table_sql
                }

                fn column_names() -> &'static [&'static str] {
                    &[#(#column_names),*]
                }

                fn column_definitions() -> &'static [(&'static str, &'static str, bool)] {
                    &[#((#col_def_names, #col_def_types, #col_def_nullables)),*]
                }

                #translation_model_methods
            }

            // Values align 1:1 with `OfflineModel::column_names()`.
            impl #offline::store::OfflineRecord for #name {
                fn primary_key_column() -> &'static str {
                    #pk_column
                }

                fn to_values(&self) -> ::std::vec::Vec<#offline::db::Value> {
                    ::std::vec![ #(#to_value_exprs),* ]
                }

                fn from_row(
                    row: &#offline::db::Row,
                ) -> ::core::result::Result<Self, #offline::db::OfflineDbError> {
                    #[allow(unused_variables)]
                    let decode = |__col: &str| #offline::db::OfflineDbError::Decode {
                        model: #type_name.to_string(),
                        message: __col.to_string(),
                    };
                    #[allow(unused_variables)]
                    let decode_msg = |__col: &str, __cause: &dyn ::core::fmt::Display| {
                        #offline::db::OfflineDbError::Decode {
                            model: #type_name.to_string(),
                            message: ::std::format!("{}: {}", __col, __cause),
                        }
                    };
                    ::core::result::Result::Ok(Self {
                        #(#from_row_extractions,)*
                        #(#from_row_skipped,)*
                    })
                }

                #translation_record_methods
            }
        }
    } else {
        quote! {}
    };

    // `HasDatabaseAffinity` has one `Database`: the online backend wins over offline.
    let affinity_marker = if struct_attrs.backends.sql {
        Some(quote! { #core::model::SqlDb })
    } else if struct_attrs.backends.cql {
        Some(quote! { #core::model::CqlDb })
    } else if struct_attrs.backends.offline {
        Some(quote! { #core::model::Offline })
    } else {
        None
    };
    let affinity_impl = affinity_marker.map(|marker| {
        quote! {
            impl #core::model::HasDatabaseAffinity for #name {
                type Database = #marker;
            }
        }
    });

    // The online impls are emitted only where the backend crate is in the graph, so a plain
    // `#[model(sql)]` compiles everywhere; the condition is a feature, not the target, since more
    // surfaces than the browser own no database.
    let database_impl = match backend_emit {
        BackendEmit::Always => database_impl,
        BackendEmit::UnderFeature(feature) => {
            gate_items(database_impl, quote! { #[cfg(feature = #feature)] })
        }
        BackendEmit::Never => quote! {},
    };

    // A typed `connection = C` is otherwise referenced only inside the backend impl, so where that
    // is absent a touch keeps `use my::C;` from firing `unused_imports`.
    let connection_touch = struct_attrs.connection.as_ref().and_then(|path| {
        let touch = quote! {
            const _: () = {
                const fn __assert_connection<__C: #core::model::Connection>() {}
                __assert_connection::<#path>();
            };
        };
        match backend_emit {
            BackendEmit::Always => None,
            BackendEmit::UnderFeature(feature) => Some(quote! {
                #[cfg(not(feature = #feature))]
                #touch
            }),
            BackendEmit::Never => Some(touch),
        }
    });

    Ok(quote! {
        #model_impl

        #database_impl

        #offline_impl

        #server_syncable_impl

        #affinity_impl

        #connection_touch

        #labels_impl
    })
}

/// How a model's online-backend impls reach their driver crate in this build, if at all.
#[derive(Clone, Copy)]
enum BackendEmit {
    /// The consumer depends on the backend crate directly: unconditional impls.
    Always,
    /// Through the `fundamental` umbrella: impls under the feature it forwards.
    UnderFeature(&'static str),
    /// No backend crate in the graph, or no online backend: no impls.
    Never,
}

impl BackendEmit {
    /// Reads a resolved [`crate::derives::backend_impl_target`] for the umbrella module `feature`.
    fn of(
        target: &Option<(proc_macro2::TokenStream, Option<proc_macro2::TokenStream>)>,
        feature: &'static str,
    ) -> Self {
        match target {
            Some((_, None)) => Self::Always,
            Some((_, Some(_))) => Self::UnderFeature(feature),
            None => Self::Never,
        }
    }
}

/// Prefixes every top-level item in `items` with `cfg`, keeping each in place so path resolution
/// is unchanged.
///
/// # Panics
/// Never on the derive's own output; a block that is not items becomes a `compile_error!`.
fn gate_items(
    items: proc_macro2::TokenStream,
    cfg: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if items.is_empty() {
        return items;
    }
    match syn::parse2::<syn::File>(items) {
        Ok(file) => {
            let gated = file.items.into_iter().map(|item| {
                quote! {
                    #cfg
                    #item
                }
            });
            quote! { #(#gated)* }
        }
        Err(error) => {
            let message =
                format!("internal error: model backend impl was not a sequence of items: {error}");
            quote! { ::core::compile_error!(#message); }
        }
    }
}

/// The `TableSchema` additions for a syncable table's overlay, plus on YSQL an `updated_at` index
/// for change-feeds; empty when the model is not syncable.
#[expect(clippy::too_many_arguments, reason = "readability baseline")]
fn sync_overlay_schema_adds(
    db: &proc_macro2::TokenStream,
    syncable: bool,
    table_name: &str,
    ts_type: &str,
    int_type: &str,
    bool_type: &str,
    with_index: bool,
) -> proc_macro2::TokenStream {
    if !syncable {
        return quote! {};
    }
    let column = |name: &str, ty: &str| {
        quote! {
            __table.add_column(#db::migration::TableColumn {
                name: #name.to_string(),
                data_type: #ty.to_string(),
                nullable: false,
                default: ::core::option::Option::None,
                primary_key: false,
                unique: false,
            });
        }
    };
    let updated_at = column("updated_at", ts_type);
    let version = column("version", int_type);
    let tombstone = column("_tombstone", bool_type);
    let index = if with_index {
        let idx_name = format!("{table_name}_sync_feed_idx");
        quote! {
            __table.add_index(#db::migration::TableIndex {
                name: #idx_name.to_string(),
                columns: ::std::vec!["updated_at".to_string()],
                unique: false,
                condition: ::core::option::Option::None,
                key: #db::migration::KeyPart::parts(&[("updated_at", ::core::option::Option::None)]),
                include: ::std::vec::Vec::new(),
                constraint: false,
            });
        }
    } else {
        quote! {}
    };
    quote! {
        #updated_at
        #version
        #tombstone
        #index
    }
}

/// The `ModelSchema::translation_schema` for backend `marker`; nothing without `Localized` text.
fn translation_schema_item(
    db: &proc_macro2::TokenStream,
    marker: proc_macro2::TokenStream,
    migration_db: &proc_macro2::TokenStream,
    localized: bool,
) -> proc_macro2::TokenStream {
    if !localized {
        return quote! {};
    }
    quote! {
        fn translation_schema() -> ::core::option::Option<#db::migration::TableSchema> {
            ::core::option::Option::Some(
                <Self as #db::migration::ModelSchema<#marker>>::table_schema()
                    .translations(#migration_db),
            )
        }
    }
}

/// Emits the `ModelSchema<D>` impl for one backend, from the same [`ColumnSpec`]s as the DDL.
#[allow(clippy::too_many_arguments)]
#[expect(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
fn generate_model_schema_impl(
    db: &proc_macro2::TokenStream,
    core: &proc_macro2::TokenStream,
    name: &Ident,
    marker: &proc_macro2::TokenStream,
    migration_db: &proc_macro2::TokenStream,
    table_name: &str,
    specs: &[ColumnSpec],
    soft_delete: bool,
    soft_delete_type: &str,
    primary_key: &[String],
    partition_key: Option<&[String]>,
    clustering_key: Option<&[(String, ClusteringOrder)]>,
    extra_adds: proc_macro2::TokenStream,
    extra_items: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let column_adds: Vec<proc_macro2::TokenStream> = specs
        .iter()
        .map(|s| {
            let col = &s.name;
            let data_type = s.storage_type_expr.clone().unwrap_or_else(|| {
                let literal = &s.storage_type;
                quote! { #literal }
            });
            let nullable = s.nullable;
            let primary_key = s.primary_key;
            let default = match &s.default {
                Some(default) => quote! { ::core::option::Option::Some(#default.to_string()) },
                None => quote! { ::core::option::Option::None },
            };
            quote! {
                __table.add_column(#db::migration::TableColumn {
                    name: #col.to_string(),
                    data_type: #data_type.to_string(),
                    nullable: #nullable,
                    default: #default,
                    primary_key: #primary_key,
                    unique: false,
                });
            }
        })
        .collect();

    let soft_delete_add = if soft_delete {
        quote! {
            __table.add_column(#db::migration::TableColumn {
                name: "deleted_at".to_string(),
                data_type: #soft_delete_type.to_string(),
                nullable: true,
                default: ::core::option::Option::None,
                primary_key: false,
                unique: false,
            });
        }
    } else {
        quote! {}
    };

    let key_columns: Vec<proc_macro2::TokenStream> = specs
        .iter()
        .filter(|spec| spec.primary_key)
        .map(|spec| {
            let column = &spec.name;
            let split = split_tokens(core, spec.shard);
            quote! { (#column, #split) }
        })
        .collect();
    let set_primary_key = if primary_key.is_empty() {
        quote! {}
    } else {
        quote! {
            __table.set_primary_key(::std::vec![ #(#primary_key.to_string()),* ]);
            __table.set_key(#db::migration::KeyPart::parts(&[ #(#key_columns),* ]));
        }
    };

    let set_partition_key = match partition_key {
        Some(cols) if !cols.is_empty() => {
            quote! { __table.set_partition_key(::std::vec![ #(#cols.to_string()),* ]); }
        }
        _ => quote! {},
    };

    let set_clustering_key = match clustering_key {
        Some(cols) if !cols.is_empty() => {
            let entries: Vec<proc_macro2::TokenStream> = cols
                .iter()
                .map(|(col, order)| {
                    let order_tokens = match order {
                        ClusteringOrder::Asc => {
                            quote! { #core::model::cql::ClusteringOrder::Asc }
                        }
                        ClusteringOrder::Desc => {
                            quote! { #core::model::cql::ClusteringOrder::Desc }
                        }
                    };
                    quote! { (#col.to_string(), #order_tokens) }
                })
                .collect();
            quote! { __table.set_clustering_key(::std::vec![ #(#entries),* ]); }
        }
        _ => quote! {},
    };

    quote! {
        impl #db::migration::ModelSchema<#marker> for #name {
            const MIGRATION_DATABASE: #db::migration::MigrationDatabase = #migration_db;

            fn table_schema() -> #db::migration::TableSchema {
                let mut __table = #db::migration::TableSchema::new(#table_name);
                #(#column_adds)*
                #soft_delete_add
                #extra_adds
                #set_primary_key
                #set_partition_key
                #set_clustering_key
                __table
            }

            #extra_items
        }
    }
}

/// The inherent label impl: `entity_label`, `entity_label_plural` and `field_label`.
fn generate_labels_impl(
    core: &proc_macro2::TokenStream,
    name: &Ident,
    type_name: &str,
    struct_attrs: &StructAttrs,
    field_infos: &[FieldInfo<'_>],
) -> proc_macro2::TokenStream {
    let entity_key = fundamental_i18n_keys::model_entity_key(type_name);
    let entity_label = struct_attrs
        .label
        .clone()
        .unwrap_or_else(|| fundamental_i18n_keys::humanize_pascal(type_name));
    let plural_key = fundamental_i18n_keys::model_entity_plural_key(type_name);
    let plural_label = struct_attrs
        .label_plural
        .clone()
        .unwrap_or_else(|| fundamental_i18n_keys::pluralize_label(&entity_label));

    let mut field_arms = Vec::new();
    for f in field_infos {
        let field_name = f.ident.to_string();
        let key = fundamental_i18n_keys::model_field_key(type_name, &field_name);
        let label = f
            .attrs
            .label
            .clone()
            .unwrap_or_else(|| fundamental_i18n_keys::humanize_snake(&field_name));
        field_arms.push(quote! {
            #field_name => #core::i18n::translate(#key, &[], || ::std::borrow::Cow::Borrowed(#label))
        });
    }

    quote! {
        impl #name {
            /// The localized singular entity label (e.g. "User").
            pub fn entity_label() -> ::std::borrow::Cow<'static, str> {
                #core::i18n::translate(#entity_key, &[], || ::std::borrow::Cow::Borrowed(#entity_label))
            }

            /// The localized plural entity label (e.g. "Users").
            pub fn entity_label_plural() -> ::std::borrow::Cow<'static, str> {
                #core::i18n::translate(#plural_key, &[], || ::std::borrow::Cow::Borrowed(#plural_label))
            }

            /// The localized label for a field by name (e.g. `"email"` → "Email
            /// Address"). Unknown field names return the name unchanged.
            pub fn field_label(field: &str) -> ::std::borrow::Cow<'_, str> {
                match field {
                    #(#field_arms,)*
                    _ => ::std::borrow::Cow::Borrowed(field),
                }
            }
        }
    }
}

/// `Option<ShardKind>` as the generated code spells it.
fn split_tokens(
    core: &proc_macro2::TokenStream,
    split: Option<fundamental_core::model::sql::ShardKind>,
) -> proc_macro2::TokenStream {
    let variant = match split {
        None => return quote! { ::core::option::Option::None },
        Some(fundamental_core::model::sql::ShardKind::Hash) => quote!(Hash),
        Some(fundamental_core::model::sql::ShardKind::Asc) => quote!(Asc),
        Some(fundamental_core::model::sql::ShardKind::Desc) => quote!(Desc),
    };
    quote! { ::core::option::Option::Some(#core::model::sql::ShardKind::#variant) }
}

#[cfg(test)]
mod tests {
    use super::model_impl;
    use syn::{DeriveInput, parse_quote};

    fn expand(input: DeriveInput) -> String {
        model_impl(&input)
            .expect("the Model derive expands")
            .to_string()
    }

    #[test]
    fn a_table_name_must_be_spelled_as_the_database_resolves_it() {
        use super::is_canonical_table_name;
        for name in ["posts", "_audit", "fundamental_inbox", "layer2_entries"] {
            assert!(is_canonical_table_name(name), "{name}");
        }
        for name in [
            "",
            "Posts",
            "Fundamental_inbox",
            " fundamental_inbox",
            "public.posts",
            "2fa",
            "café",
        ] {
            assert!(!is_canonical_table_name(name), "{name:?}");
        }
    }

    #[test]
    fn placement_writers_and_acceptances_reach_the_table_schema() {
        let expanded = expand(parse_quote! {
            #[model(sql, table = "import_events", tablets = 8, written_by(role = "importer"))]
            #[model(accept(hot_shard = "read newest first by one importer"))]
            struct ImportEvent {
                #[model(id, shard = hash)]
                organisation_id: uuid::Uuid,
                #[model(id, shard = desc)]
                at: i64,
            }
        });

        for emitted in [
            "__table . add_writer (\"importer\")",
            "__table . set_tablets (8)",
            "__table . set_colocated (false)",
            "model :: sql :: Lint :: HotShard , \"read newest first by one importer\"",
        ] {
            assert!(expanded.contains(emitted), "{emitted} in {expanded}");
        }
    }

    #[test]
    fn tablets_on_a_range_split_key_are_refused() {
        let input: DeriveInput = parse_quote! {
            #[model(sql, table = "events", tablets = 8)]
            struct Event {
                #[model(id, shard = asc)]
                at: i64,
            }
        };

        let error = model_impl(&input).expect_err("a range-split key is split at values");

        assert!(
            error.to_string().contains("splits a table by hash"),
            "{error}"
        );
    }

    #[test]
    fn a_table_folded_to_a_framework_table_is_refused() {
        let input: DeriveInput = parse_quote! {
            #[model(sql, table = "Fundamental_inbox")]
            struct Ledger {
                #[model(id)]
                effect: String,
            }
        };
        let error = model_impl(&input).expect_err("a folded spelling of a framework table");
        assert!(
            error
                .to_string()
                .contains("not a table name as the database resolves it"),
            "{error}"
        );
    }

    /// Expansions here take the umbrella arm, since `fundamental_macros` depends on no backend
    /// crate.
    #[test]
    fn the_sql_backend_impl_is_gated_on_the_backend_capability() {
        let input: DeriveInput = parse_quote! {
            #[model(sql, table = "post")]
            struct Post {
                #[model(id)]
                id: String,
                title: String,
            }
        };
        let out = expand(input);
        assert!(
            out.contains("cfg (feature = \"sql\")"),
            "the sql backend impl must be gated on the backend capability:\n{out}"
        );
        assert!(out.contains("TYPE_NAME"), "{out}");
    }

    #[test]
    fn the_cql_backend_impl_is_gated_on_its_own_capability() {
        let input: DeriveInput = parse_quote! {
            #[model(cql, table = "event")]
            struct Event {
                #[model(partition_key)]
                tenant_id: String,
                #[model(id, clustering_key)]
                id: String,
            }
        };
        let out = expand(input);
        assert!(out.contains("cfg (feature = \"cql\")"), "{out}");
        assert!(!out.contains("cfg (feature = \"sql\")"), "{out}");
    }

    /// The condition is capability, never the target.
    #[test]
    fn no_model_emits_a_target_gate() {
        let backendless: DeriveInput = parse_quote! {
            struct Dto {
                #[model(id)]
                id: String,
                title: String,
            }
        };
        let bound: DeriveInput = parse_quote! {
            #[model(sql, table = "post", connection = Reports)]
            struct Post {
                #[model(id)]
                id: String,
            }
        };
        let offline_synced: DeriveInput = parse_quote! {
            #[model(sql, offline, syncable)]
            struct Note {
                #[model(id)]
                id: String,
            }
        };
        for input in [backendless, bound, offline_synced] {
            let out = expand(input);
            assert!(!out.contains("target_arch"), "{out}");
        }
    }

    #[test]
    fn a_backendless_model_emits_only_the_portable_half() {
        let input: DeriveInput = parse_quote! {
            struct Dto {
                #[model(id)]
                id: String,
                title: String,
            }
        };
        let out = expand(input);
        assert!(!out.contains("cfg (feature"), "{out}");
        assert!(out.contains("TYPE_NAME"), "{out}");
    }

    /// Without the touch, `use my::C;` fires `unused_imports` on a build with no database.
    #[test]
    fn a_typed_connection_is_touched_where_the_backend_impl_is_absent() {
        let input: DeriveInput = parse_quote! {
            #[model(sql, table = "post", connection = Reports)]
            struct Post {
                #[model(id)]
                id: String,
            }
        };
        let out = expand(input);
        assert!(
            out.contains("cfg (not (feature = \"sql\"))"),
            "a typed connection must be touched where the impl is not emitted:\n{out}"
        );
        assert!(out.contains("__assert_connection"), "{out}");
        assert!(out.contains("Reports"), "{out}");
    }

    #[test]
    fn a_domain_string_connection_gets_no_touch() {
        let input: DeriveInput = parse_quote! {
            #[model(sql, table = "post", domain = "reports")]
            struct Post {
                #[model(id)]
                id: String,
            }
        };
        let out = expand(input);
        assert!(
            !out.contains("__assert_connection"),
            "a domain-string model needs no connection touch:\n{out}"
        );
    }

    #[test]
    fn a_backendless_typed_connection_is_touched_unconditionally() {
        let input: DeriveInput = parse_quote! {
            #[model(offline, connection = HolderVault)]
            struct Row {
                #[model(id)]
                id: String,
            }
        };
        let out = expand(input);
        assert!(out.contains("__assert_connection"), "{out}");
        assert!(out.contains("HolderVault"), "{out}");
        assert!(!out.contains("cfg (not (feature"), "{out}");
    }
}
