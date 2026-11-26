//! `migrations!` — an owner's migrations, generated from its models, embedded from its crate.

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, LitStr, Path, Token, bracketed};

use crate::derives::{core_path, db_path};

/// `owner = "…", models = [Row, …]`, `retired = ["table", …]` once a table is gone,
/// `needs_createrole = ["0002_…", …]` for the files that make a role without looking it up, and
/// `drops_by_hand = ["0002_…", …]` for the files that drop or rename what no model declares.
struct Input {
    owner: LitStr,
    models: Vec<Path>,
    retired: Vec<LitStr>,
    needs_createrole: Vec<LitStr>,
    drops_by_hand: Vec<LitStr>,
}

impl Parse for Input {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut owner = None;
        let mut models = None;
        let mut retired = Vec::new();
        let mut needs_createrole = Vec::new();
        let mut drops_by_hand = Vec::new();
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if key == "owner" {
                owner = Some(input.parse::<LitStr>()?);
            } else if key == "models" {
                let content;
                bracketed!(content in input);
                let listed: Punctuated<Path, Token![,]> =
                    content.parse_terminated(Path::parse, Token![,])?;
                models = Some(listed.into_iter().collect());
            } else if key == "retired" {
                retired = names(input)?;
            } else if key == "needs_createrole" {
                needs_createrole = names(input)?;
            } else if key == "drops_by_hand" {
                drops_by_hand = names(input)?;
            } else {
                return Err(syn::Error::new_spanned(
                    key,
                    "`migrations!` takes `owner = \"…\"`, `models = [Row, …]`, \
                     `retired = [\"table\", …]`, `needs_createrole = [\"0002_…\", …]` and \
                     `drops_by_hand = [\"0002_…\", …]`",
                ));
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            owner: owner.ok_or_else(|| input.error("`migrations!` needs `owner = \"…\"`"))?,
            models: models.ok_or_else(|| input.error("`migrations!` needs `models = [Row, …]`"))?,
            retired,
            needs_createrole,
            drops_by_hand,
        })
    }
}

fn names(input: ParseStream<'_>) -> syn::Result<Vec<LitStr>> {
    let content;
    bracketed!(content in input);
    let listed: Punctuated<LitStr, Token![,]> =
        content.parse_terminated(|names| names.parse::<LitStr>(), Token![,])?;
    Ok(listed.into_iter().collect())
}

pub(crate) fn migrations(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    match syn::parse::<Input>(input).and_then(|input| expand(&input)) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand(input: &Input) -> syn::Result<TokenStream> {
    let dir = std::env::var("CARGO_MANIFEST_DIR")
        .map_err(|_| syn::Error::new_spanned(&input.owner, "cargo sets CARGO_MANIFEST_DIR"))?;
    let root = std::path::Path::new(&dir).join("migrations");
    let files = migration_files(&root);
    let db = db_path();
    let core = core_path();
    let owner = &input.owner;
    let migrations = files.iter().map(|(name, path)| {
        quote! { #db::migration::EmbeddedMigration::new(#name, ::core::include_str!(#path)) }
    });
    let tables = input.models.iter().map(|model| {
        quote! { <#model as #db::migration::ModelSchema<#core::model::SqlDb>>::table_schema }
    });
    let companions = input.models.iter().map(|model| {
        quote! { <#model as #db::migration::ModelSchema<#core::model::SqlDb>>::translation_schema }
    });
    let snapshot = snapshot(&root, &input.owner)?;
    let declared = declared(input);
    Ok(quote! {
        #db::migration::EmbeddedMigrations::sql(
            #owner,
            &[ #(#migrations),* ],
            &[ #(#tables),* ],
        )
        .companions(&[ #(#companions),* ])
        .generated(#snapshot, #dir)
        #declared
    })
}

/// What the owner says of its files beside its models: the tables it retired, the files that
/// need `CREATEROLE` and the files that drop by hand.
fn declared(input: &Input) -> TokenStream {
    let retired = &input.retired;
    let needs_createrole = &input.needs_createrole;
    let drops_by_hand = &input.drops_by_hand;
    quote! {
        .retired(&[ #(#retired),* ])
        .needing_createrole(&[ #(#needs_createrole),* ])
        .dropping_by_hand(&[ #(#drops_by_hand),* ])
    }
}

/// The owner's committed `migrations/schema.json`, embedded; refused when it has none yet.
fn snapshot(root: &std::path::Path, owner: &LitStr) -> syn::Result<TokenStream> {
    let snapshot_path = root.join("schema.json");
    if !snapshot_path.exists() {
        return Err(syn::Error::new_spanned(
            owner,
            format!(
                "{} has no migrations/schema.json: create it holding `{{}}`, then write the \
                 first snapshot with `fun migrate generate baseline`",
                owner.value()
            ),
        ));
    }
    let snapshot_path = snapshot_path.display().to_string();
    Ok(quote! { ::core::include_str!(#snapshot_path) })
}

/// Every `<name>.up.sql` in `root/sql`, by name, in order, each with its path.
fn migration_files(root: &std::path::Path) -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = std::fs::read_dir(root.join("sql"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    let name = entry.file_name().into_string().ok()?;
                    let stem = name.strip_suffix(".up.sql")?.to_owned();
                    Some((stem, entry.path().display().to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::Input;

    #[test]
    fn an_owner_names_the_tables_it_retired() {
        let input: Input = syn::parse_quote!(
            owner = "acme_mail",
            models = [MailOutboxRow],
            retired = ["fundamental_mail_drafts", "fundamental_mail_old"]
        );
        let retired: Vec<String> = input.retired.iter().map(syn::LitStr::value).collect();
        assert_eq!(retired, ["fundamental_mail_drafts", "fundamental_mail_old"]);
    }

    #[test]
    fn an_owner_names_the_files_that_need_createrole() {
        let input: Input = syn::parse_quote!(
            owner = "newsroom",
            models = [Article],
            needs_createrole = ["0002_www_reads_published"]
        );
        let needs: Vec<String> = input
            .needs_createrole
            .iter()
            .map(syn::LitStr::value)
            .collect();
        assert_eq!(needs, ["0002_www_reads_published"]);
    }

    #[test]
    fn an_owner_names_the_files_that_drop_by_hand() {
        let input: Input = syn::parse_quote!(
            owner = "portfolio",
            models = [Project],
            drops_by_hand = ["0002_portfolio_featured"]
        );
        let named: Vec<String> = input.drops_by_hand.iter().map(syn::LitStr::value).collect();
        assert_eq!(named, ["0002_portfolio_featured"]);
    }
}
