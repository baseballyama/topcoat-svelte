//! The [`SvelteComponent`] handle and the [`Island`] it renders.

use serde::Serialize;
use topcoat::context::Cx;
use topcoat::view::{NodeViewParts, PartsWriter, View};

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
        let (props_json, comment) = self.props_json(props);
        // Under `ssr`, server-render the component into the island; the same
        // `props_json` string that seeds hydration is the render input. Any
        // engine error degrades to a client-rendered (empty) island rather than
        // failing the response. Without the feature this is always CSR, so the
        // HTML is byte-identical to a client-only build.
        let (ssr_attr, server_html) = self.server_render_island(&module_url, &props_json);
        let html = format!(
            "{comment}{}",
            hydration_root(&module_url, ssr_attr, &server_html, &props_json)
        );
        Island { html }
    }

    /// Renders the component as a **full HTML document** whose entire body is the
    /// component tree, with the `#[page]` Rust function playing the role of
    /// SvelteKit's `load()`: it computes `props` and hands them to the page.
    ///
    /// The returned [`Page`] sits in node position inside
    /// [`view!`](topcoat::view::view) and emits `<!doctype html>` through
    /// `<html>` with a head carrying [`script`](crate::script)'s import
    /// map + loader (plus the component's `<svelte:head>` when server-rendered)
    /// and a body whose sole child is the hydration root.
    ///
    /// Add Rust-supplied head content with [`Page::with_head`]; it composes with
    /// any `<svelte:head>` output. `page` works with and without the `ssr`
    /// feature: with it the document arrives server-rendered and hydrates;
    /// without it the body root is empty and mounts on the client, following the
    /// same props path.
    ///
    /// `cx` is accepted for symmetry with [`island`](Self::island) and forward
    /// compatibility; it is currently unused at construction.
    #[must_use]
    pub fn page(&self, cx: &Cx, props: &impl Serialize) -> Page {
        let _ = cx;
        let module_url = self.module_url();
        let (props_json, comment) = self.props_json(props);
        let (ssr_attr, head, body) = self.server_render_page(&module_url, &props_json);
        Page {
            comment,
            script: crate::script::markup(),
            svelte_head: head,
            extra_head: None,
            root: hydration_root(&module_url, ssr_attr, &body, &props_json),
        }
    }

    /// Serializes `props` into the XSS-safe JSON string embedded in the
    /// hydration root, returning it together with an HTML comment (empty on
    /// success) that explains a serialization failure. On failure the props fall
    /// back to `{}` and the error is logged, matching the island contract.
    fn props_json(&self, props: &impl Serialize) -> (String, String) {
        match to_script_json(props) {
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
        }
    }

    /// Server-renders the island's body when the `ssr` feature is enabled,
    /// returning the `data-tcs-ssr` attribute (with a leading space) and the
    /// server HTML. On any engine error, or without the feature, returns empty
    /// strings so the island falls back to client rendering.
    #[cfg(feature = "ssr")]
    fn server_render_island(&self, module_url: &str, props_json: &str) -> (&'static str, String) {
        match crate::ssr::render_island(module_url, props_json) {
            Ok(html) => (" data-tcs-ssr", html),
            Err(err) => {
                self.report_ssr_error(&err);
                ("", String::new())
            }
        }
    }

    #[cfg(not(feature = "ssr"))]
    fn server_render_island(&self, _module_url: &str, _props_json: &str) -> (&'static str, String) {
        ("", String::new())
    }

    /// Server-renders the page when the `ssr` feature is enabled, returning the
    /// `data-tcs-ssr` attribute (with a leading space), the `<svelte:head>`
    /// content, and the body HTML. On any engine error, or without the feature,
    /// returns empty strings so the page degrades to a client-mounted document.
    #[cfg(feature = "ssr")]
    fn server_render_page(
        &self,
        module_url: &str,
        props_json: &str,
    ) -> (&'static str, String, String) {
        match crate::ssr::render_page(module_url, props_json) {
            Ok(output) => (" data-tcs-ssr", output.head, output.body),
            Err(err) => {
                self.report_ssr_error(&err);
                ("", String::new(), String::new())
            }
        }
    }

    #[cfg(not(feature = "ssr"))]
    fn server_render_page(
        &self,
        _module_url: &str,
        _props_json: &str,
    ) -> (&'static str, String, String) {
        ("", String::new(), String::new())
    }

    #[cfg(feature = "ssr")]
    fn report_ssr_error(&self, err: &str) {
        eprintln!(
            "topcoat-svelte: failed to server-render component {}: {err}",
            self.name
        );
    }
}

/// Builds the island-shaped hydration root shared by [`SvelteComponent::island`]
/// and [`SvelteComponent::page`]: the `data-tcs-island` div carrying the module
/// URL, the (possibly empty) server HTML, and the props script. Keeping both
/// paths on this one template is what lets the client loader hydrate a page with
/// zero page-specific code.
fn hydration_root(module_url: &str, ssr_attr: &str, server_html: &str, props_json: &str) -> String {
    format!(
        "<div data-tcs-island{ssr_attr} data-tcs-module=\"{module_url}\" \
         style=\"display:contents\">{server_html}\
         <script type=\"application/json\">{props_json}</script></div>"
    )
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

/// A full HTML document rendered from a single Svelte component tree, produced
/// by [`SvelteComponent::page`]. Usable in node position inside
/// [`view!`](topcoat::view::view) -- typically as the whole body of a `#[page]`
/// function.
pub struct Page {
    /// An HTML comment explaining a props-serialization failure, or empty.
    comment: String,
    /// The import map + loader markup from [`crate::script::markup`].
    script: String,
    /// The component's `<svelte:head>` content (empty without server rendering).
    svelte_head: String,
    /// Optional Rust-supplied head content, added via [`Page::with_head`].
    extra_head: Option<View>,
    /// The island-shaped hydration root that spans the document body.
    root: String,
}

impl Page {
    /// Adds Rust-supplied content to the document `<head>`, composing after
    /// [`script`](crate::script)'s output and the component's `<svelte:head>`.
    ///
    /// Build the head with [`view!`](topcoat::view::view) and pass the resulting
    /// `View` (unwrap the macro's `Result` with `?`):
    ///
    /// ```ignore
    /// # use topcoat::{Result, context::Cx, view::view};
    /// # use topcoat_svelte::SvelteComponent;
    /// # async fn example(cx: &Cx, page: &SvelteComponent) -> Result {
    /// view! { cx =>
    ///     (page.page(cx, &()).with_head(view! { cx =>
    ///         <meta name="description" content="Rust + Svelte">
    ///     }?))
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn with_head(mut self, head: View) -> Self {
        self.extra_head = Some(head);
        self
    }
}

impl NodeViewParts for Page {
    fn into_view_parts(self, cx: &Cx, parts: &mut PartsWriter<'_>) {
        // Every string here is either a fixed template, hash-only URLs, server
        // HTML that already went through the SSR path, or props escaped by
        // `to_script_json`, so emitting them unescaped is safe. Only the
        // Rust-supplied `extra_head` goes through the normal (escaping) view
        // path, exactly as if written inline in `view!`.
        parts.push_str_unescaped(self.comment);
        parts.push_str_unescaped("<!doctype html><html><head>");
        parts.push_str_unescaped(self.script);
        parts.push_str_unescaped(self.svelte_head);
        if let Some(head) = self.extra_head {
            head.into_view_parts(cx, parts);
        }
        parts.push_str_unescaped("</head><body>");
        parts.push_str_unescaped(self.root);
        parts.push_str_unescaped("</body></html>");
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

    /// Without the `ssr` feature, `page` emits a full document whose body root
    /// is an empty (client-mounted) hydration root -- no server markup, no
    /// `data-tcs-ssr` -- with the runtime wired into the head.
    #[cfg(not(feature = "ssr"))]
    #[tokio::test]
    async fn page_document_structure_without_ssr() {
        use topcoat::view::view;

        let cx = CxTestBuilder::new().build();
        let cx = &cx;
        static COMPONENT: SvelteComponent = SvelteComponent::new("Doc", "0123456789abcdef");
        let html = view! { cx => (COMPONENT.page(cx, &serde_json::json!({ "count": 3 }))) }
            .unwrap()
            .render(cx);

        assert!(html.starts_with("<!doctype html><html><head>"), "{html}");
        assert!(html.contains("type=\"importmap\""));
        assert!(html.contains("/_topcoat-svelte/loader.js?v="));
        assert!(html.contains("</head><body>"));
        // Empty client-mounted root spanning the body.
        assert!(!html.contains("data-tcs-ssr"), "{html}");
        assert!(html.contains(
            "<div data-tcs-island data-tcs-module=\"/_topcoat-svelte/c/Doc-0123456789abcdef.js\" \
             style=\"display:contents\">\
             <script type=\"application/json\">{\"count\":3}</script></div>"
        ));
        assert!(html.trim_end().ends_with("</body></html>"), "{html}");
    }

    /// `with_head` places Rust-supplied head content inside `<head>`, after the
    /// runtime scripts.
    #[cfg(not(feature = "ssr"))]
    #[tokio::test]
    async fn page_with_head_composes_extra_head() {
        use topcoat::view::view;

        let cx = CxTestBuilder::new().build();
        let cx = &cx;
        static COMPONENT: SvelteComponent = SvelteComponent::new("Doc", "abc123");
        let extra = view! { cx => <meta name="description" content="hi"> }.unwrap();
        let html = view! { cx => (COMPONENT.page(cx, &serde_json::json!({})).with_head(extra)) }
            .unwrap()
            .render(cx);

        let head_end = html.find("</head>").unwrap();
        let loader_at = html.find("loader.js").unwrap();
        let meta_at = html.find("name=\"description\"").unwrap();
        assert!(
            loader_at < meta_at,
            "extra head must follow the runtime: {html}"
        );
        assert!(
            meta_at < head_end,
            "extra head must be inside <head>: {html}"
        );
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
