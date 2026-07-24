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
}

impl CompiledModule {
    /// Creates a compiled-module record. Called by the [`svelte!`](crate::svelte)
    /// macro with the component name, a content hash of `js`, and the compiled
    /// client JavaScript.
    #[must_use]
    pub const fn new(name: &'static str, hash: &'static str, js: &'static str) -> Self {
        Self { name, hash, js }
    }

    /// The component name (the PascalCase file stem).
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The content hash of the compiled JavaScript.
    #[must_use]
    pub fn hash(&self) -> &'static str {
        self.hash
    }

    /// The compiled client JavaScript.
    #[must_use]
    pub fn js(&self) -> &'static str {
        self.js
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
