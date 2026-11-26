//! `#[derive(Messages)]`: a type said through its crate's messages.

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Fields, Path, parse_macro_input};

use crate::derives::case::to_snake_case;

pub(crate) fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let module = module_of(input)?;
    let data = match &input.data {
        Data::Enum(data) => data,
        Data::Struct(data) => match &data.fields {
            Fields::Unit => return Ok(noun(input, &module)),
            Fields::Named(fields) => return field_words(input, &module, fields),
            Fields::Unnamed(_) => {
                return Err(syn::Error::new_spanned(
                    name,
                    "Messages takes a unit struct (a type's name) or named fields (one message each)",
                ));
            }
        },
        Data::Union(_) => {
            return Err(syn::Error::new_spanned(
                name,
                "Messages can only be derived for an enum or a struct",
            ));
        }
    };
    let messages = messages_path();
    let mut arms = Vec::new();
    let mut checks = Vec::new();
    for variant in &data.variants {
        let (arm, check) = variant_arm(name, &module, &messages, variant)?;
        arms.push(arm);
        checks.extend(check);
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        #(#checks)*

        impl #impl_generics #name #ty_generics #where_clause {
            /// Its words, as its message says them in the reader's language.
            #[must_use]
            pub fn message(&self) -> #messages::Message {
                match self {
                    #(#arms,)*
                }
            }
        }

        impl #impl_generics ::core::fmt::Display for #name #ty_generics #where_clause {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.message(), f)
            }
        }
    })
}

/// A variant's `message()` arm, with a compile-time check that its named fields are the message's inputs in order.
fn variant_arm(
    name: &syn::Ident,
    module: &Path,
    messages: &proc_macro2::TokenStream,
    variant: &syn::Variant,
) -> syn::Result<(proc_macro2::TokenStream, Option<proc_macro2::TokenStream>)> {
    let ident = &variant.ident;
    let function = function_ident(&to_snake_case(&ident.to_string()));
    match &variant.fields {
        Fields::Unit => Ok((quote!(Self::#ident => #module::#function()), None)),
        Fields::Named(fields) => {
            let bound: Vec<&syn::Ident> = fields
                .named
                .iter()
                .filter_map(|field| field.ident.as_ref())
                .collect();
            let names: Vec<String> = bound
                .iter()
                .map(|field| {
                    let spelled = field.to_string();
                    spelled.strip_prefix("r#").unwrap_or(&spelled).to_owned()
                })
                .collect();
            let inputs =
                format_ident!("{}", to_snake_case(&ident.to_string()).to_ascii_uppercase());
            let said = format!(
                "{name}::{ident}'s fields must be the inputs its message `{function}` declares, by name and in order"
            );
            Ok((
                quote! {
                    Self::#ident { #(#bound),* } =>
                        #module::#function(#(::core::clone::Clone::clone(#bound)),*)
                },
                Some(quote! {
                    const _: () = ::core::assert!(
                        #messages::__inputs_are(#module::__inputs::#inputs, &[#(#names),*]),
                        #said
                    );
                }),
            ))
        }
        Fields::Unnamed(_) => Err(syn::Error::new_spanned(
            variant,
            "Messages takes a variant with no fields or named fields, each an input of its message",
        )),
    }
}

/// A message's function name as Rust spells it, keywords raw (`r#do`).
fn function_ident(name: &str) -> syn::Ident {
    syn::parse_str::<syn::Ident>(name).unwrap_or_else(|_| format_ident!("r#{name}"))
}

fn module_of(input: &DeriveInput) -> syn::Result<Path> {
    let name = &input.ident;
    input
        .attrs
        .iter()
        .find(|attr| attr.path().is_ident("messages"))
        .ok_or_else(|| {
            syn::Error::new_spanned(
                name,
                "Messages needs #[messages(path::to::module)]: the generated module holding one message per variant",
            )
        })?
        .parse_args()
}

/// `Noun` for a unit struct, from the module's five messages.
fn noun(input: &DeriveInput, module: &Path) -> proc_macro2::TokenStream {
    let name = &input.ident;
    let messages = messages_path();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let forms = ["one", "many", "the", "new_one", "this"].map(|form| {
        let function = format_ident!("{form}");
        quote! {
            fn #function() -> #messages::Message {
                #module::#function()
            }
        }
    });
    quote! {
        impl #impl_generics #messages::Noun for #name #ty_generics #where_clause {
            #(#forms)*
        }
    }
}

/// `FieldWords` for a struct: one message per field.
fn field_words(
    input: &DeriveInput,
    module: &Path,
    fields: &syn::FieldsNamed,
) -> syn::Result<proc_macro2::TokenStream> {
    let name = &input.ident;
    let messages = messages_path();
    let mut labels = Vec::new();
    let mut values = Vec::new();
    for field in &fields.named {
        let Some(ident) = &field.ident else { continue };
        let spelled = ident.to_string();
        let spelled = spelled.strip_prefix("r#").unwrap_or(&spelled).to_owned();
        let function = format_ident!("{}", ident);
        labels.push(quote!(#spelled => ::core::option::Option::Some(#module::#function())));
        if is_variant(field)? {
            let ty = option_inner(&field.ty).unwrap_or(&field.ty);
            values.push(quote! {
                #spelled => <#ty as #messages::__Variant>::from_value(value).map(|said| said.message())
            });
        }
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics #messages::FieldWords for #name #ty_generics #where_clause {
            fn field(name: &str) -> ::core::option::Option<#messages::Message> {
                match name {
                    #(#labels,)*
                    _ => ::core::option::Option::None,
                }
            }

            fn value(field: &str, value: &str) -> ::core::option::Option<#messages::Message> {
                let _ = value;
                match field {
                    #(#values,)*
                    _ => ::core::option::Option::None,
                }
            }
        }
    })
}

fn is_variant(field: &syn::Field) -> syn::Result<bool> {
    let mut variant = false;
    for attr in field
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("messages"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("variant") {
                variant = true;
                Ok(())
            } else {
                Err(meta
                    .error("a field takes #[messages(variant)]: its values said by its Variant"))
            }
        })?;
    }
    Ok(variant)
}

fn option_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    match args.args.first()? {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

/// The messages runtime, as a direct dependency or through the umbrella.
fn messages_path() -> proc_macro2::TokenStream {
    match crate_name("fundamental_messages") {
        Ok(FoundCrate::Itself) => quote!(crate),
        Ok(FoundCrate::Name(n)) => {
            let ident = format_ident!("{n}");
            quote!(::#ident)
        }
        Err(_) => match crate_name("fundamental") {
            Ok(FoundCrate::Name(n)) => {
                let ident = format_ident!("{n}");
                quote!(::#ident::messages)
            }
            _ => quote!(::fundamental::messages),
        },
    }
}
