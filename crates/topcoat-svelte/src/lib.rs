//! Use [Svelte 5](https://svelte.dev) components as client-rendered **islands**
//! inside [Topcoat](https://crates.io/crates/topcoat) apps.
//!
//! Components are compiled to JavaScript at Rust build time by
//! [rsvelte](https://github.com/baseballyama/rsvelte), so an application needs
//! only the Rust toolchain -- no Node.js and no bundler. The compiled modules
//! and the vendored Svelte client runtime are served from memory by the app
//! itself.
//!
//! # Overview
//!
//! Three pieces work together:
//!
//! - [`svelte!`] compiles a `.svelte` file and yields a [`SvelteComponent`].
//! - [`SvelteComponent::island`] renders the component as an island node inside
//!   [`view!`](topcoat::view::view), seeded with props from Rust.
//! - [`SvelteComponent::page`] renders a **whole HTML document** from one Svelte
//!   component tree, with the `#[page]` function as SvelteKit's `load()`.
//! - [`script`] emits the import map and island loader (place it in `<head>`),
//!   and the [`serve`] route serves the runtime and compiled modules.
//!
//! ```ignore
//! use topcoat::prelude::*;
//! use topcoat_svelte::{svelte, SvelteComponent};
//!
//! static COUNTER: SvelteComponent = svelte!("./Counter.svelte");
//!
//! #[page("/")]
//! async fn index(cx: &Cx) -> Result {
//!     view! {
//!         <html>
//!             <head>(topcoat_svelte::script())</head>
//!             <body>(COUNTER.island(cx, &serde_json::json!({ "count": 3 })))</body>
//!         </html>
//!     }
//! }
//! ```
//!
//! Register [`serve`] on the router with `.route(topcoat_svelte::serve)`.
//!
//! # Rendering modes
//!
//! By default islands are client-rendered: they render empty on the server and
//! mount in the browser. Enabling the `ssr` feature server-renders each island's
//! HTML (through an embedded JavaScript engine) and hydrates it on the client.
//! [`SvelteComponent::page`] renders a whole document the same way and works in
//! both modes. See the crate's `docs/islands.md` and `docs/pages.md` guides and
//! `DESIGN.md` for details.

#![forbid(unsafe_code)]

mod component;
mod escape;
mod registry;
mod runtime;
mod script;
mod serve;
#[cfg(feature = "ssr")]
mod ssr;

pub use component::*;
pub use registry::*;
pub use script::*;
pub use serve::*;

pub use topcoat_svelte_macro::svelte;

/// The URL namespace every `topcoat-svelte` asset is served under.
pub(crate) const NAMESPACE: &str = "/_topcoat-svelte";

/// Implementation details used by the [`svelte!`] macro's expansion. Not public
/// API.
#[doc(hidden)]
pub mod __private {
    pub use inventory;
}
