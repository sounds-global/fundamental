use crate::expand::canonical_root;
use fundamental_canonical::__private::write_string;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Fields, LitByteStr, LitStr, parse_quote};

/// Expand `#[derive(CanonicalJson)]`, or say why the struct has no canonical encoding.
#[expect(clippy::too_many_lines, reason = "readability baseline")]
pub(crate) fn expand(input: &DeriveInput) -> Result<TokenStream, syn::Error> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "CanonicalJson is derived for structs with named fields: an enum has no one canonical shape",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "CanonicalJson is derived for structs with named fields: a member needs a name",
        ));
    };

    let mut members = Vec::with_capacity(fields.named.len());
    for field in &fields.named {
        let Some(ident) = &field.ident else {
            return Err(syn::Error::new_spanned(field, "a member needs a name"));
        };
        let name = member_name(field)?
            .unwrap_or_else(|| ident.to_string().trim_start_matches("r#").to_owned());
        if let Some((_, other, _)) = members
            .iter()
            .find(|(existing, _, _): &&(String, _, _)| *existing == name)
        {
            return Err(syn::Error::new_spanned(
                field,
                format!(
                    "two members are named `{name}` (the other is `{other}`): one name must mean one member"
                ),
            ));
        }
        members.push((name, ident.clone(), field.ty.clone()));
    }
    // RFC 8785 §3.2.3 orders members by UTF-16 code unit.
    members.sort_by(|(left, _, _), (right, _, _)| left.encode_utf16().cmp(right.encode_utf16()));

    let root = canonical_root();
    let writes = members.iter().map(|(name, ident, _)| {
        let mut key = Vec::with_capacity(name.len() + 3);
        write_string(&mut key, name);
        key.push(b':');
        let key = LitByteStr::new(&key, Span::call_site());
        quote! {
            #root::__private::CanonicalMember::write_member(&self.#ident, #key, out, &mut first);
        }
    });

    let mut generics = input.generics.clone();
    if generics.type_params().next().is_some() {
        let where_clause = generics.make_where_clause();
        for (_, _, ty) in &members {
            where_clause
                .predicates
                .push(parse_quote!(#ty: #root::__private::CanonicalMember));
        }
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    let ident = &input.ident;
    Ok(quote! {
        impl #impl_generics #root::CanonicalJson for #ident #type_generics #where_clause {
            fn write_canonical_json(&self, out: &mut ::std::vec::Vec<u8>) {
                out.push(b'{');
                let mut first = true;
                #(#writes)*
                let _ = first;
                out.push(b'}');
            }
        }
    })
}

fn member_name(field: &syn::Field) -> Result<Option<String>, syn::Error> {
    let mut name = None;
    for attribute in field
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("canonical"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                let value: LitStr = meta.value()?.parse()?;
                name = Some(value.value());
                Ok(())
            } else {
                Err(meta.error("the one option is `rename = \"…\"`"))
            }
        })?;
    }
    Ok(name)
}
