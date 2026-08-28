//! `#[rbun::class]` and `#[rbun::methods]`, modelled on `#[rquickjs::class]`
//! and `#[rquickjs::methods]`.
//!
//! ```ignore
//! #[rbun::class]
//! struct Counter { n: i64 }
//!
//! #[rbun::methods(rename_all = "camelCase")]
//! impl Counter {
//!     #[qjs(constructor)]
//!     fn new<'js>(ctx: Ctx<'js>, args: Rest<Value<'js>>) -> rbun::Result<Self> { .. }
//!     fn increment_by<'js>(&mut self, ctx: Ctx<'js>, n: i64) -> rbun::Result<i64> { .. }
//!     #[qjs(static)]
//!     fn zero() -> i64 { 0 }
//! }
//! Class::<Counter>::define(&ctx.globals())?;
//! ```
//!
//! Like rquickjs, `#[rbun::class]` only records the class name (and
//! `frozen`); the struct still needs `Trace` and `JsLifetime`, which can be
//! derived with `#[derive(rbun::Trace, rbun::JsLifetime)]`. `#[rbun::methods]`
//! implements `JsClass` with a prototype holding the methods and a
//! constructor built from the `#[qjs(constructor)]` function. Conventions:
//! method lifetimes must be named `'js`; `#[qjs(rename = "…")]`,
//! `#[qjs(static)]`, `#[qjs(get)]` and `#[qjs(skip)]` are honoured.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, ImplItem, ItemImpl, ItemStruct, Pat, ReturnType, Type, parse_macro_input};

#[proc_macro_attribute]
pub fn class(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemStruct);
    let ident = &input.ident;
    let mut name = ident.to_string();
    let mut frozen = false;
    if !attr.is_empty() {
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("rename") {
                let value: syn::LitStr = meta.value()?.parse()?;
                name = value.value();
                Ok(())
            } else if meta.path.is_ident("frozen") {
                frozen = true;
                Ok(())
            } else if meta.path.is_ident("crate") {
                let _ = meta.value().and_then(|v| v.parse::<syn::Expr>());
                Ok(())
            } else {
                Err(meta.error("unsupported #[rbun::class] option"))
            }
        });
        parse_macro_input!(attr with parser);
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mutability = if frozen { quote!(::rbun::Readable) } else { quote!(::rbun::Writable) };
    quote! {
        #input
        impl #impl_generics ::rbun::JsClassName for #ident #ty_generics #where_clause {
            const NAME: &'static str = #name;
        }
        impl #impl_generics ::rbun::class::ClassMutability for #ident #ty_generics #where_clause {
            type Mutable = #mutability;
        }
    }
    .into()
}

#[derive(Default)]
struct MethodAttrs {
    constructor: bool,
    is_static: bool,
    skip: bool,
    getter: bool,
    rename: Option<String>,
}

fn parse_qjs_attrs(attrs: &mut Vec<syn::Attribute>) -> syn::Result<MethodAttrs> {
    let mut out = MethodAttrs::default();
    let mut kept = Vec::new();
    for attr in attrs.drain(..) {
        if attr.path().is_ident("qjs") {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("constructor") {
                    out.constructor = true;
                } else if meta.path.is_ident("static") {
                    out.is_static = true;
                } else if meta.path.is_ident("skip") {
                    out.skip = true;
                } else if meta.path.is_ident("get") {
                    out.getter = true;
                } else if meta.path.is_ident("rename") {
                    let value: syn::LitStr = meta.value()?.parse()?;
                    out.rename = Some(value.value());
                } else if meta.path.is_ident("set") || meta.path.is_ident("enumerable") || meta.path.is_ident("configurable") {
                    // accepted for source compatibility
                } else {
                    return Err(meta.error("unsupported #[qjs] option"));
                }
                Ok(())
            })?;
        } else {
            kept.push(attr);
        }
    }
    *attrs = kept;
    Ok(out)
}

fn to_camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for ch in name.chars() {
        if ch == '_' {
            upper = true;
        } else if upper {
            out.extend(ch.to_uppercase());
            upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

#[proc_macro_attribute]
pub fn methods(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemImpl);
    let mut rename_all: Option<String> = None;
    if !attr.is_empty() {
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("rename_all") {
                let value: syn::LitStr = meta.value()?.parse()?;
                rename_all = Some(value.value());
                Ok(())
            } else if meta.path.is_ident("crate") {
                let _ = meta.value().and_then(|v| v.parse::<syn::Expr>());
                Ok(())
            } else {
                Err(meta.error("unsupported #[rbun::methods] option"))
            }
        });
        parse_macro_input!(attr with parser);
    }

    let self_ty = &input.self_ty;
    let mut constructor: Option<proc_macro2::TokenStream> = None;
    let mut methods: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut statics: Vec<proc_macro2::TokenStream> = Vec::new();

    for item in &mut input.items {
        let ImplItem::Fn(method) = item else { continue };
        let attrs = match parse_qjs_attrs(&mut method.attrs) {
            Ok(a) => a,
            Err(e) => return e.to_compile_error().into(),
        };
        if attrs.skip || !matches!(method.vis, syn::Visibility::Public(_) | syn::Visibility::Inherited) {
            continue;
        }
        let fn_ident = &method.sig.ident;
        let mut receiver = None;
        let mut params: Vec<(syn::Ident, Type)> = Vec::new();
        for (i, arg) in method.sig.inputs.iter().enumerate() {
            match arg {
                FnArg::Receiver(r) => receiver = Some(r.clone()),
                FnArg::Typed(pat_type) => {
                    let ident = match &*pat_type.pat {
                        Pat::Ident(p) => format_ident!("__arg_{}", p.ident.to_string().trim_start_matches('_')),
                        _ => format_ident!("__arg_{}", i),
                    };
                    params.push((ident, (*pat_type.ty).clone()));
                }
            }
        }
        let param_names: Vec<_> = params.iter().map(|(n, _)| n).collect();
        let param_types: Vec<_> = params.iter().map(|(_, t)| t).collect();
        let js_name = attrs.rename.clone().unwrap_or_else(|| match rename_all.as_deref() {
            Some("camelCase") => to_camel_case(&fn_ident.to_string()),
            _ => fn_ident.to_string(),
        });

        if attrs.constructor {
            constructor = Some(quote! {
                let __constructor = ::rbun::Constructor::new_class::<#self_ty, _, _>(*__ctx, |#(#param_names: #param_types),*| -> ::rbun::Result<::rbun::Value<'js>> {
                    let __ctx = ::rbun::function::current_ctx()?;
                    let __value: #self_ty = ::rbun::class::IntoClassResult::into_class_result(<#self_ty>::#fn_ident(#(#param_names),*))?;
                    Ok(::rbun::Class::<#self_ty>::instance(__ctx, __value)?.into_value())
                })?;
                Ok(Some(__constructor))
            });
            continue;
        }

        let returns_unit = matches!(method.sig.output, ReturnType::Default);

        match receiver {
            Some(_) if !attrs.is_static => {
                let call = if returns_unit {
                    quote! { { <#self_ty>::#fn_ident(__instance, #(#param_names),*); () } }
                } else {
                    quote! { <#self_ty>::#fn_ident(__instance, #(#param_names),*) }
                };
                let register = if attrs.getter { quote!(getter) } else { quote!(method) };
                methods.push(quote! {
                    let __builder = __builder.#register(#js_name, |__this: ::rbun::This<::rbun::Value<'js>>, #(#param_names: #param_types),*| -> ::rbun::Result<::rbun::Value<'js>> {
                        let __cx = *__this.0.ctx();
                        ::rbun::with_instance::<#self_ty, _>(&__this.0, |__instance| {
                            ::rbun::IntoJs::into_js(#call, &__cx)
                        })
                    })?;
                });
            }
            _ => {
                let call = if returns_unit {
                    quote! { { <#self_ty>::#fn_ident(#(#param_names),*); () } }
                } else {
                    quote! { <#self_ty>::#fn_ident(#(#param_names),*) }
                };
                statics.push(quote! {
                    __constructor.set(#js_name, ::rbun::Function::new(*__ctx, |#(#param_names: #param_types),*| #call)?.with_name(#js_name)?)?;
                });
            }
        }
    }

    let constructor_body = match constructor {
        Some(body) => quote! {
            let __ctx = ctx;
            #body
        },
        None => quote! { Ok(None) },
    };
    let statics_body = if statics.is_empty() {
        quote! {}
    } else {
        quote! {
            let __ctx = ctx;
            if let Some(__constructor) = &__result {
                #(#statics)*
            }
        }
    };

    quote! {
        #input
        impl<'js> ::rbun::JsClass<'js> for #self_ty {
            const NAME: &'static str = <#self_ty as ::rbun::JsClassName>::NAME;
            type Mutable = <#self_ty as ::rbun::class::ClassMutability>::Mutable;

            fn prototype(ctx: &::rbun::Ctx<'js>) -> ::rbun::Result<Option<::rbun::Object<'js>>> {
                let __builder = ::rbun::PrototypeBuilder::new(ctx)?;
                #(#methods)*
                Ok(Some(__builder.build()))
            }

            fn constructor(ctx: &::rbun::Ctx<'js>) -> ::rbun::Result<Option<::rbun::Constructor<'js>>> {
                let __result: Option<::rbun::Constructor<'js>> = (|| -> ::rbun::Result<Option<::rbun::Constructor<'js>>> { #constructor_body })()?;
                #statics_body
                Ok(__result)
            }
        }
    }
    .into()
}

/// `#[derive(Trace)]` — a no-op tracer (values are protected individually).
#[proc_macro_derive(Trace, attributes(qjs))]
pub fn derive_trace(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::DeriveInput);
    let ident = &input.ident;
    let mut generics = input.generics.clone();
    let has_js = generics.lifetimes().any(|l| l.lifetime.ident == "js");
    if !has_js {
        generics.params.insert(0, syn::parse_quote!('js));
    }
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    let (_, ty_generics, _) = input.generics.split_for_impl();
    quote! {
        impl #impl_generics ::rbun::Trace<'js> for #ident #ty_generics #where_clause {
            fn trace<'a>(&self, _tracer: ::rbun::Tracer<'a, 'js>) {}
        }
    }
    .into()
}

/// `#[derive(JsLifetime)]` — `Changed<'to>` substitutes the type's `'js`
/// lifetime (or is the type itself when it has no lifetime parameters).
#[proc_macro_derive(JsLifetime)]
pub fn derive_js_lifetime(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as syn::DeriveInput);
    let ident = &input.ident;
    let lifetimes: Vec<_> = input.generics.lifetimes().cloned().collect();
    if lifetimes.len() > 1 {
        return syn::Error::new_spanned(&input.ident, "JsLifetime can only be derived for types with at most one lifetime parameter")
            .to_compile_error()
            .into();
    }
    let type_params: Vec<_> = input.generics.type_params().map(|p| &p.ident).collect();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    if let Some(lifetime) = lifetimes.first() {
        let js = &lifetime.lifetime;
        let changed = quote! { #ident<'to #(, #type_params)*> };
        quote! {
            unsafe impl #impl_generics ::rbun::JsLifetime<#js> for #ident #ty_generics #where_clause {
                type Changed<'to> = #changed;
            }
        }
        .into()
    } else {
        let mut generics = input.generics.clone();
        generics.params.insert(0, syn::parse_quote!('js));
        let (impl_generics, _, where_clause) = generics.split_for_impl();
        quote! {
            unsafe impl #impl_generics ::rbun::JsLifetime<'js> for #ident #ty_generics #where_clause {
                type Changed<'to> = #ident #ty_generics;
            }
        }
        .into()
    }
}
