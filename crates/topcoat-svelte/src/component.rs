//! The [`SvelteComponent`] handle and the [`Island`] it renders.

use serde::Serialize;
use topcoat::context::Cx;
use topcoat::view::{NodeViewParts, PartsWriter};

use crate::escape::to_script_json;

/// A handle to a Svelte component compiled by the [`svelte!`](crate::svelte)
/// macro.
///
/// Construct one at compile time and render it as a client-side island with
/// [`island`](SvelteComponent::island):
///
/// ```ignore
/// use topcoat_svelte::{svelte, SvelteComponent};
///
/// static COUNTER: SvelteComponent = svelte!("./Counter.svelte");
/// ```
///
/// The component renders nothing on the server; the browser fetches its module
/// and mounts it in place (see the crate-level docs for how the loader and route
/// fit together).
pub struct SvelteComponent {
    name: &'static str,
    hash: &'static str,
}

impl SvelteComponent {
    /// Creates a handle from a component name and the content hash of its
    /// compiled JavaScript. Called by the [`svelte!`](crate::svelte) macro; not
    /// meant to be constructed by hand.
    #[must_use]
    pub const fn new(name: &'static str, hash: &'static str) -> Self {
        Self { name, hash }
    }

    /// The component name (the PascalCase file stem).
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The URL the compiled module is served from, including its content hash.
    #[must_use]
    pub fn module_url(&self) -> String {
        format!("{}/c/{}-{}.js", crate::NAMESPACE, self.name, self.hash)
    }

    /// Renders the component as an island seeded with `props`.
    ///
    /// The returned [`Island`] sits in node position inside
    /// [`view!`](topcoat::view::view). By default it renders an empty placeholder
    /// plus the serialized props, and the browser fetches the module and mounts
    /// the component into it. With the `ssr` feature enabled, the component is
    /// server-rendered into the placeholder and the browser hydrates it instead.
    ///
    /// `props` is any [`Serialize`] value (commonly a
    /// [`serde_json::json!`](serde_json::json) object). `cx` is accepted for
    /// forward compatibility and is currently unused.
    #[must_use]
    pub fn island(&self, cx: &Cx, props: &impl Serialize) -> Island {
        let _ = cx;
        let module_url = self.module_url();
        let (props_json, comment) = match to_script_json(props) {
            Ok(json) => (json, String::new()),
            Err(err) => {
                eprintln!(
                    "topcoat-svelte: failed to serialize props for component {}: {err}",
                    self.name
                );
                (
                    "{}".to_owned(),
                    format!(
                        "<!-- topcoat-svelte: failed to serialize props: {} -->",
                        sanitize_comment(&err.to_string())
                    ),
                )
            }
        };
        // Under `ssr`, server-render the component into the island; the same
        // `props_json` string that seeds hydration is the render input. Any
        // engine error degrades to a client-rendered (empty) island rather than
        // failing the response. Without the feature this is always CSR, so the
        // HTML is byte-identical to a client-only build.
        let (ssr_attr, server_html) = self.server_render(&module_url, &props_json);
        let html = format!(
            "{comment}<div data-tcs-island{ssr_attr} data-tcs-module=\"{module_url}\" \
             style=\"display:contents\">{server_html}\
             <script type=\"application/json\">{props_json}</script></div>"
        );
        Island { html }
    }

    /// Server-renders the island's markup when the `ssr` feature is enabled,
    /// returning the `data-tcs-ssr` attribute (with a leading space) and the
    /// server HTML. On any engine error, or without the feature, returns empty
    /// strings so the island falls back to client rendering.
    #[cfg(feature = "ssr")]
    fn server_render(&self, module_url: &str, props_json: &str) -> (&'static str, String) {
        match crate::ssr::render_island(module_url, props_json) {
            Ok(html) => (" data-tcs-ssr", html),
            Err(err) => {
                eprintln!(
                    "topcoat-svelte: failed to server-render component {}: {err}",
                    self.name
                );
                ("", String::new())
            }
        }
    }

    #[cfg(not(feature = "ssr"))]
    fn server_render(&self, _module_url: &str, _props_json: &str) -> (&'static str, String) {
        ("", String::new())
    }
}

/// A client-rendered Svelte island, produced by
/// [`SvelteComponent::island`]. Usable in node position inside
/// [`view!`](topcoat::view::view).
pub struct Island {
    html: String,
}

impl NodeViewParts for Island {
    fn into_view_parts(self, _cx: &Cx, parts: &mut PartsWriter<'_>) {
        // `html` is assembled here from a fixed template, a hash-only URL, and
        // props escaped by `to_script_json`, so it is safe to emit verbatim.
        parts.push_str_unescaped(self.html);
    }
}

/// Neutralizes `-->` so a serialization error message cannot close the HTML
/// comment it is embedded in.
fn sanitize_comment(message: &str) -> String {
    message.replace("--", "- -").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use topcoat::context::CxTestBuilder;

    use crate::SvelteComponent;

    #[test]
    fn serve_url_matches_macro() {
        // The `svelte!` macro bakes child module URLs into the compiled JS using
        // the literal prefix `/_topcoat-svelte/c/` (see the macro's `graph`
        // module). `module_url` must produce exactly that shape, or a rewritten
        // child import would 404.
        static COMPONENT: SvelteComponent = SvelteComponent::new("Child", "0011223344556677");
        assert_eq!(
            COMPONENT.module_url(),
            "/_topcoat-svelte/c/Child-0011223344556677.js"
        );
    }

    /// Without the `ssr` feature an island is always client-rendered, so its
    /// HTML must be exactly the client-only placeholder -- no `data-tcs-ssr`, no
    /// server markup. This pins that output byte-for-byte.
    #[cfg(not(feature = "ssr"))]
    #[test]
    fn island_html_is_byte_identical_without_ssr() {
        let cx = CxTestBuilder::new().build();
        static COMPONENT: SvelteComponent = SvelteComponent::new("Counter", "0123456789abcdef");
        let island = COMPONENT.island(&cx, &serde_json::json!({ "count": 3 }));
        assert_eq!(
            island.html,
            "<div data-tcs-island data-tcs-module=\"/_topcoat-svelte/c/Counter-0123456789abcdef.js\" \
             style=\"display:contents\">\
             <script type=\"application/json\">{\"count\":3}</script></div>"
        );
    }

    #[test]
    fn island_html_has_marker_module_and_props() {
        let cx = CxTestBuilder::new().build();
        static COMPONENT: SvelteComponent = SvelteComponent::new("Counter", "0123456789abcdef");
        let island = COMPONENT.island(&cx, &serde_json::json!({ "count": 3 }));
        assert!(island.html.contains("data-tcs-island"));
        assert!(
            island
                .html
                .contains("data-tcs-module=\"/_topcoat-svelte/c/Counter-0123456789abcdef.js\"")
        );
        assert!(island.html.contains("style=\"display:contents\""));
        assert!(
            island
                .html
                .contains("<script type=\"application/json\">{\"count\":3}</script>")
        );
    }

    #[test]
    fn island_escapes_malicious_props() {
        let cx = CxTestBuilder::new().build();
        static COMPONENT: SvelteComponent = SvelteComponent::new("Counter", "abc123");
        let island = COMPONENT.island(
            &cx,
            &serde_json::json!({ "x": "</script><script>alert(1)</script>" }),
        );
        assert!(!island.html.contains("</script><script>alert"));
        assert!(island.html.contains("\\u003c/script\\u003e"));
    }

    /// A value whose `Serialize` impl always fails, to exercise `island`'s
    /// fallback path.
    struct AlwaysFailsToSerialize;

    impl serde::Serialize for AlwaysFailsToSerialize {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("intentional failure for test"))
        }
    }

    #[test]
    fn island_falls_back_to_empty_props_on_serialize_failure() {
        let cx = CxTestBuilder::new().build();
        static COMPONENT: SvelteComponent = SvelteComponent::new("Counter", "abc123");
        let island = COMPONENT.island(&cx, &AlwaysFailsToSerialize);
        // The HTML output still renders a valid island with empty props, plus
        // an explanatory comment -- logging the error (via `eprintln!`) does
        // not change this fallback output.
        assert!(
            island
                .html
                .contains("<!-- topcoat-svelte: failed to serialize props:")
        );
        assert!(
            island
                .html
                .contains("<script type=\"application/json\">{}</script>")
        );
        assert!(island.html.contains("data-tcs-island"));
    }
}
