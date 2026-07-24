//! The `svelte!` macro for [`topcoat-svelte`](https://crates.io/crates/topcoat-svelte).
//!
//! Compiles a Svelte 5 component to client JavaScript at build time through
//! rsvelte and expands to a `topcoat_svelte::SvelteComponent`.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use proc_macro::TokenStream;
use quote::quote;
use rsvelte_core::compiler::{CompileOptions, CssMode, GenerateMode, compile};
use sha2::{Digest, Sha256};
use syn::{LitStr, parse_macro_input};

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
/// becomes a `compile_error!`. The component is registered so
/// [`serve`](../topcoat_svelte/constant.serve.html) can serve its module.
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
    let path = resolve_path(&arg.value())?;
    let abs = path
        .to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {}", path.display()))?
        .to_owned();

    let name = component_name(&path)?;
    let source = std::fs::read_to_string(&path)
        .map_err(|err| format!("could not read {}: {err}", path.display()))?;

    let options = CompileOptions {
        generate: GenerateMode::Client,
        css: CssMode::Injected,
        name: Some(name.clone()),
        filename: Some(abs.clone()),
        ..Default::default()
    };
    let compiled = compile(&source, options)
        .map_err(|err| format!("failed to compile {}: {err}", path.display()))?;
    let js = compiled.js.code;

    let hash = short_hash(&js);

    // `include_str!` with the absolute path re-runs this macro (and thus the
    // compile) whenever the component source changes.
    Ok(quote! {
        {
            const _: &str = ::core::include_str!(#abs);
            ::topcoat_svelte::__private::inventory::submit! {
                ::topcoat_svelte::CompiledModule::new(#name, #hash, #js)
            }
            ::topcoat_svelte::SvelteComponent::new(#name, #hash)
        }
    })
}

/// Resolves the macro argument to an existing absolute path, mirroring
/// `asset!`'s resolution rules.
fn resolve_path(arg: &str) -> Result<PathBuf, String> {
    let joined = if arg.starts_with("./") || arg.starts_with("../") {
        source_dir()?.join(arg)
    } else {
        let path = Path::new(arg);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            manifest_dir()?.join(arg)
        }
    };
    joined
        .canonicalize()
        .map_err(|err| format!("could not find {}: {err}", joined.display()))
}

/// The directory of the source file that invoked the macro.
fn source_dir() -> Result<PathBuf, String> {
    let file = proc_macro::Span::call_site()
        .local_file()
        .ok_or("could not determine the calling source file")?;
    let file = if file.is_absolute() {
        file
    } else {
        std::env::current_dir()
            .map_err(|err| format!("could not read the current directory: {err}"))?
            .join(file)
    };
    file.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "the calling source file has no parent directory".to_owned())
}

fn manifest_dir() -> Result<PathBuf, String> {
    std::env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .map_err(|_| "CARGO_MANIFEST_DIR is not set".to_owned())
}

/// Derives a PascalCase component name from a file stem, so `counter.svelte`
/// and `my-widget.svelte` become `Counter` and `MyWidget`.
///
/// The derived name must be ASCII: a non-ASCII name would produce a
/// `module_url` that does not match the percent-encoded path browsers send,
/// causing the served module to 404. Rename the file to ASCII instead.
fn component_name(path: &Path) -> Result<String, String> {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("could not read a file name from {}", path.display()))?;

    let mut name = String::with_capacity(stem.len());
    let mut capitalize_next = true;
    for ch in stem.chars() {
        if ch.is_alphanumeric() {
            if capitalize_next {
                name.extend(ch.to_uppercase());
            } else {
                name.push(ch);
            }
            capitalize_next = false;
        } else {
            capitalize_next = true;
        }
    }

    if name.is_empty() || name.starts_with(|c: char| c.is_numeric()) {
        return Err(format!(
            "{} does not yield a valid component name",
            path.display()
        ));
    }
    if !name.is_ascii() {
        return Err(format!(
            "{} yields a non-ASCII component name ({name:?}); rename the file to \
             use only ASCII characters",
            path.display()
        ));
    }
    Ok(name)
}

/// The first 16 hex characters of the SHA-256 of `js`.
fn short_hash(js: &str) -> String {
    let digest = Sha256::digest(js.as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::component_name;
    use std::path::Path;

    #[test]
    fn pascal_cases_simple_stems() {
        assert_eq!(
            component_name(Path::new("counter.svelte")).unwrap(),
            "Counter"
        );
        assert_eq!(
            component_name(Path::new("my-widget.svelte")).unwrap(),
            "MyWidget"
        );
        assert_eq!(
            component_name(Path::new("my_widget.svelte")).unwrap(),
            "MyWidget"
        );
    }

    #[test]
    fn rejects_names_starting_with_a_digit() {
        assert!(component_name(Path::new("42widget.svelte")).is_err());
    }

    #[test]
    fn rejects_empty_names() {
        assert!(component_name(Path::new("---.svelte")).is_err());
    }

    #[test]
    fn rejects_non_ascii_stems() {
        let err = component_name(Path::new("café.svelte")).unwrap_err();
        assert!(
            err.contains("non-ASCII"),
            "expected a non-ASCII error, got: {err}"
        );

        let err = component_name(Path::new("ボタン.svelte")).unwrap_err();
        assert!(
            err.contains("non-ASCII"),
            "expected a non-ASCII error, got: {err}"
        );
    }
}
