//! Server-side rendering tests, gated on the `ssr` feature. They compile
//! fixtures through `svelte!`, render their islands, and assert the island HTML
//! carries the server-rendered markup and `data-tcs-ssr`, that a module graph is
//! resolved through the engine's loader, and that a render error degrades to a
//! client-rendered island.
#![cfg(feature = "ssr")]

use topcoat::context::{Cx, CxTestBuilder};
use topcoat::view::view;
use topcoat_svelte::{SvelteComponent, svelte};

static WIDGET: SvelteComponent = svelte!("./fixtures/ssr/Widget.svelte");
static BOOM: SvelteComponent = svelte!("./fixtures/ssr/Boom.svelte");
static PARENT: SvelteComponent = svelte!("./fixtures/graph/Parent.svelte");

async fn render_island(cx: &Cx, component: &SvelteComponent, props: &serde_json::Value) -> String {
    view! { cx => (component.island(cx, props)) }
        .unwrap()
        .render(cx)
}

#[tokio::test]
async fn widget_island_is_server_rendered() {
    let cx = CxTestBuilder::new().build();
    let html = render_island(&cx, &WIDGET, &serde_json::json!({ "count": 5 })).await;

    // The island is marked for hydration and carries the exact server body,
    // including Svelte's hydration boundary markers.
    assert!(html.contains("data-tcs-ssr"));
    assert!(
        html.contains("<!--[--><button>count 5</button><!--]-->"),
        "missing server body in: {html}"
    );

    // The server markup comes before the props script, and the props are still
    // embedded for hydration.
    let body_at = html.find("<button>count 5</button>").unwrap();
    let props_at = html.find("<script type=\"application/json\">").unwrap();
    assert!(
        body_at < props_at,
        "server HTML must precede the props script"
    );
    assert!(html.contains("<script type=\"application/json\">{\"count\":5}</script>"));
}

#[tokio::test]
async fn graph_island_ssrs_with_children_resolved() {
    let cx = CxTestBuilder::new().build();
    let html = render_island(&cx, &PARENT, &serde_json::json!({ "count": 2 })).await;

    assert!(html.contains("data-tcs-ssr"));
    // The child component rendered inline (its prop interpolated)...
    assert!(html.contains("item 2"), "child not rendered in: {html}");
    // ...and the grandchild it imports rendered too, proving the whole graph
    // resolved through the engine's module loader.
    assert!(
        html.contains("<span class=\"grandchild\">"),
        "grandchild not rendered in: {html}"
    );
}

#[tokio::test]
async fn ssr_error_falls_back_to_client_rendering() {
    let cx = CxTestBuilder::new().build();

    // A render that throws degrades to a CSR island: no SSR marker, no server
    // markup, but still a valid island with its props for the client to mount.
    let html = render_island(&cx, &BOOM, &serde_json::json!({ "boom": true })).await;
    assert!(
        !html.contains("data-tcs-ssr"),
        "should have fallen back: {html}"
    );
    assert!(!html.contains("<p>safe</p>"));
    assert!(html.contains("data-tcs-island"));
    assert!(html.contains("<script type=\"application/json\">{\"boom\":true}</script>"));

    // The same component renders normally when it does not throw.
    let ok = render_island(&cx, &BOOM, &serde_json::json!({ "boom": false })).await;
    assert!(ok.contains("data-tcs-ssr"));
    assert!(
        ok.contains("<p>safe</p>"),
        "expected server markup in: {ok}"
    );
}
