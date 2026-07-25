//! The in-memory registry of compiled Svelte components.
//!
//! The [`svelte!`](crate::svelte) macro submits one [`CompiledModule`] per
//! component through [`inventory`], so every component compiled anywhere in the
//! dependency graph is collected at startup and served by
//! [`serve`](crate::serve) without touching the filesystem.

use std::{collections::HashMap, sync::OnceLock};

/// A Svelte component compiled to client JavaScript, registered by the
/// [`svelte!`](crate::svelte) macro.
///
/// This is created only by the macro; application code interacts with the
/// resulting [`SvelteComponent`](crate::SvelteComponent) instead.
pub struct CompiledModule {
    name: &'static str,
    hash: &'static str,
    js: &'static str,
    server_js: &'static str,
}

impl CompiledModule {
    /// Creates a compiled-module record. Called by the [`svelte!`](crate::svelte)
    /// macro with the component name, a content hash of the client `js`, the
    /// compiled client JavaScript, and the compiled server JavaScript (used only
    /// when the `ssr` feature is enabled).
    #[must_use]
    pub const fn new(
        name: &'static str,
        hash: &'static str,
        js: &'static str,
        server_js: &'static str,
    ) -> Self {
        Self {
            name,
            hash,
            js,
            server_js,
        }
    }

    /// The component name (the PascalCase file stem).
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The content hash of the compiled client JavaScript.
    #[must_use]
    pub fn hash(&self) -> &'static str {
        self.hash
    }

    /// The compiled client JavaScript.
    #[must_use]
    pub fn js(&self) -> &'static str {
        self.js
    }

    /// The compiled server JavaScript, used to server-render the component when
    /// the `ssr` feature is enabled.
    #[must_use]
    pub fn server_js(&self) -> &'static str {
        self.server_js
    }

    /// The served filename, `{name}-{hash}.js`.
    #[must_use]
    pub fn filename(&self) -> String {
        format!("{}-{}.js", self.name, self.hash)
    }
}

inventory::collect!(CompiledModule);

/// Iterates over every compiled Svelte component registered in the binary.
pub fn compiled_modules() -> impl Iterator<Item = &'static CompiledModule> {
    inventory::iter::<CompiledModule>()
}

/// The filename -> JavaScript map, built once from the collected modules.
fn registry() -> &'static HashMap<String, &'static str> {
    static REGISTRY: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        compiled_modules()
            .map(|module| (module.filename(), module.js))
            .collect()
    })
}

/// Looks up a compiled module's JavaScript by its served filename
/// (`{name}-{hash}.js`).
pub(crate) fn lookup(filename: &str) -> Option<&'static str> {
    registry().get(filename).copied()
}

/// Looks up a compiled module's server JavaScript by its in-engine module key,
/// which is the component's served URL (`/_topcoat-svelte/c/{name}-{hash}.js`).
/// Used by the `ssr` engine's module loader.
#[cfg(feature = "ssr")]
pub(crate) fn server_source(module_key: &str) -> Option<&'static str> {
    static SERVER_REGISTRY: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    let registry = SERVER_REGISTRY.get_or_init(|| {
        compiled_modules()
            .map(|module| {
                let key = format!("{}/c/{}", crate::NAMESPACE, module.filename());
                (key, module.server_js)
            })
            .collect()
    });
    registry.get(module_key).copied()
}
