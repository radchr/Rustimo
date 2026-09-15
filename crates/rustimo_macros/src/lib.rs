use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::parse::Parser;
use syn::{FnArg, GenericArgument, ItemFn, Pat, ReturnType, Token, Type, parse_macro_input};

#[proc_macro_attribute]
pub fn cell(attribute: TokenStream, item: TokenStream) -> TokenStream {
    if !attribute.is_empty() {
        return syn::Error::new(proc_macro2::Span::call_site(), "#[cell] takes no arguments")
            .to_compile_error()
            .into();
    }
    let function = parse_macro_input!(item as ItemFn);
    expand_cell(function)
        .unwrap_or_else(|error| error.to_compile_error())
        .into()
}

fn expand_cell(function: ItemFn) -> Result<TokenStream2, syn::Error> {
    let signature = &function.sig;
    if signature.asyncness.is_some() || !signature.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            signature,
            "cells must be synchronous functions without type or lifetime parameters",
        ));
    }
    let ReturnType::Type(_, output_type) = &signature.output else {
        return Err(syn::Error::new_spanned(
            signature,
            "a cell needs a return type",
        ));
    };

    let name = &signature.ident;
    let module = format_ident!("__rustimo_cell_{}", name);
    let name_string = name.to_string();
    let mut loads = Vec::new();
    let mut arguments = Vec::new();
    let mut reference_specs = Vec::new();

    for input in &signature.inputs {
        let FnArg::Typed(input) = input else {
            return Err(syn::Error::new_spanned(
                input,
                "cell methods are unsupported",
            ));
        };
        let Pat::Ident(pattern) = input.pat.as_ref() else {
            return Err(syn::Error::new_spanned(
                &input.pat,
                "cell inputs must have simple names",
            ));
        };
        let Type::Reference(reference) = input.ty.as_ref() else {
            return Err(syn::Error::new_spanned(
                &input.ty,
                "cell inputs must be shared references, such as data: &DataFrame",
            ));
        };
        if reference.mutability.is_some() {
            return Err(syn::Error::new_spanned(
                &input.ty,
                "mutable cross-cell references are unsupported",
            ));
        }
        let input_name = pattern.ident.to_string();
        let input_type = &reference.elem;
        let local = format_ident!("__rustimo_input_{}", pattern.ident);
        loads.push(quote! {
            let #local = vault.get::<#input_type>(#input_name)?;
        });
        arguments.push(quote!(&*#local));
        reference_specs.push(quote! {
            ::rustimo::RefSpec {
                name: #input_name,
                expected_type: ::std::any::type_name::<#input_type>(),
            }
        });
    }

    let signal_functions = if is_ui_type(output_type) {
        quote! {
            fn update_signal(
                vault: &::rustimo::StateVault,
                value: ::rustimo::serde_json::Value,
            ) -> Result<::rustimo::View, ::rustimo::RuntimeError> {
                let ui = vault.get::<#output_type>(#name_string)?;
                ui.set_json(value)?;
                Ok(ui.to_view())
            }

            fn initial_view(vault: &::rustimo::StateVault)
                -> Result<::rustimo::View, ::rustimo::RuntimeError>
            {
                Ok(vault.get::<#output_type>(#name_string)?.to_view())
            }
        }
    } else {
        quote! {}
    };
    let signal_entry = if is_ui_type(output_type) {
        quote!(Some(update_signal))
    } else {
        quote!(None)
    };
    let view_entry = if is_ui_type(output_type) {
        quote!(Some(initial_view))
    } else {
        quote!(None)
    };
    let validate_result = if is_ui_type(output_type) {
        quote! {
            if value.name() != #name_string {
                return Err(::rustimo::RuntimeError::new(
                    "signal_name_mismatch",
                    format!("UI element '{}' must use cell name '{}'", value.name(), #name_string),
                    Some(#name_string),
                ));
            }
        }
    } else {
        quote! {}
    };

    Ok(quote! {
        #function

        #[doc(hidden)]
        mod #module {
            use super::*;

            fn run(vault: &::rustimo::StateVault)
                -> Result<::rustimo::CellExecution, ::rustimo::RuntimeError>
            {
                #(#loads)*
                let (value, view) = ::rustimo::capture_output(|| super::#name(#(#arguments),*));
                #validate_result
                Ok(::rustimo::CellExecution::new(value, view))
            }

            #signal_functions

            pub(super) fn descriptor() -> ::rustimo::CellDescriptor {
                ::rustimo::CellDescriptor {
                    name: #name_string,
                    refs: vec![#(#reference_specs),*],
                    result_type: ::std::any::type_name::<#output_type>(),
                    run,
                    update_signal: #signal_entry,
                    initial_view: #view_entry,
                }
            }
        }
    })
}

fn is_ui_type(ty: &Type) -> bool {
    let Type::Path(path) = ty else { return false };
    let Some(last) = path.path.segments.last() else {
        return false;
    };
    if last.ident != "Ui" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &last.arguments else {
        return false;
    };
    arguments
        .args
        .iter()
        .any(|arg| matches!(arg, GenericArgument::Type(_)))
}

#[proc_macro]
pub fn notebook(input: TokenStream) -> TokenStream {
    let parser = syn::punctuated::Punctuated::<syn::Ident, Token![,]>::parse_terminated;
    let names = match parser.parse(input) {
        Ok(names) => names,
        Err(error) => return error.to_compile_error().into(),
    };
    if names.is_empty() {
        return syn::Error::new(proc_macro2::Span::call_site(), "a notebook needs cells")
            .to_compile_error()
            .into();
    }
    let modules = names
        .iter()
        .map(|name| format_ident!("__rustimo_cell_{}", name));
    quote! {
        ::rustimo::Notebook::new(vec![#(#modules::descriptor()),*])
    }
    .into()
}
