//! The `svelte!` macro for [`topcoat-svelte`](https://crates.io/crates/topcoat-svelte).
//!
//! Compiles a Svelte 5 component to client JavaScript at build time through
//! rsvelte and expands to a `topcoat_svelte::SvelteComponent`. A component may
//! import other `.svelte` files with relative paths; every component reachable
//! that way is compiled and registered alongside the entry component.

#![forbid(unsafe_code)]

mod graph;
mod resolve;
mod rewrite;

use proc_macro::TokenStream;
use quote::quote;
use syn::{LitStr, parse_macro_input};

use crate::graph::Graph;
use crate::resolve::resolve_arg;

/// Compiles a Svelte component and expands to a
/// [`SvelteComponent`](../topcoat_svelte/struct.SvelteComponent.html).
///
/// The argument is a path to a `.svelte` file:
///
/// - `"./Counter.svelte"` or `"../ui/Counter.svelte"` resolves relative to the
///   source file that calls the macro.
/// - `"components/Counter.svelte"` resolves relative to the crate's
///   `CARGO_MANIFEST_DIR`.
/// - an absolute path is used as-is.
///
/// The file is read and compiled at macro-expansion time; a compile error
/// becomes a `compile_error!`. The component and every `.svelte` file it imports
/// (through relative paths) are registered so
/// [`serve`](../topcoat_svelte/constant.serve.html) can serve their modules.
///
/// ```ignore
/// static COUNTER: topcoat_svelte::SvelteComponent = topcoat_svelte::svelte!("./Counter.svelte");
/// ```
#[proc_macro]
pub fn svelte(input: TokenStream) -> TokenStream {
    let arg = parse_macro_input!(input as LitStr);
    match expand(&arg) {
        Ok(tokens) => tokens.into(),
        Err(message) => syn::Error::new(arg.span(), message)
            .to_compile_error()
            .into(),
    }
}

fn expand(arg: &LitStr) -> Result<proc_macro2::TokenStream, String> {
    let entry_path = resolve_arg(&arg.value())?;
    let graph = Graph::build(entry_path)?;

    // Every module in the graph is registered and pinned with `include_str!`.
    // The `include_str!`s make rustc re-expand this macro (and recompile the
    // graph) whenever any file in it changes.
    let registrations = graph.modules.iter().map(|module| {
        let name = &module.name;
        let hash = &module.hash;
        let js = &module.js;
        let abs = &module.abs_path;
        quote! {
            const _: &str = ::core::include_str!(#abs);
            ::topcoat_svelte::__private::inventory::submit! {
                ::topcoat_svelte::CompiledModule::new(#name, #hash, #js)
            }
        }
    });

    let entry = graph.entry();
    let entry_name = &entry.name;
    let entry_hash = &entry.hash;

    Ok(quote! {
        {
            #(#registrations)*
            ::topcoat_svelte::SvelteComponent::new(#entry_name, #entry_hash)
        }
    })
}
