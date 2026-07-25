//! Route-level tests for [`topcoat_svelte::serve`]: the vendored server runtime
//! is embedded for the `ssr` engine but must never be reachable over the public
//! asset route, while public runtime files and (under `ssr`) live rendering keep
//! working.

use topcoat::context::CxTestBuilder;
use topcoat::router::{Body, Route, StatusCode};

/// Drives the `serve` route for `path` and returns the response status.
async fn status_of(path: &str) -> StatusCode {
    let parts = http::Request::builder()
        .method("GET")
        .uri(path)
        .body(())
        .unwrap()
        .into_parts()
        .0;
    let cx = CxTestBuilder::new().request_context(parts).build();
    topcoat_svelte::serve
        .handle(&cx, Body::empty())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn server_runtime_is_not_publicly_served() {
    // The SSR-only server runtime is embedded (build.rs bundles all of
    // runtime/dist) but excluded from the public route.
    assert_eq!(
        status_of("/_topcoat-svelte/runtime/server/server.js").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status_of("/_topcoat-svelte/runtime/server/internal-server.js").await,
        StatusCode::NOT_FOUND
    );

    // Client-facing runtime files stay served.
    assert_eq!(
        status_of("/_topcoat-svelte/loader.js").await,
        StatusCode::OK
    );
    assert_eq!(
        status_of("/_topcoat-svelte/runtime/svelte.js").await,
        StatusCode::OK
    );
}

/// The server runtime being unreachable over HTTP does not stop the `ssr` engine
/// from loading it in-process and rendering.
#[cfg(feature = "ssr")]
#[tokio::test]
async fn ssr_still_renders_while_server_runtime_is_404() {
    use topcoat::view::view;
    use topcoat_svelte::{SvelteComponent, svelte};

    static WIDGET: SvelteComponent = svelte!("./fixtures/ssr/Widget.svelte");

    assert_eq!(
        status_of("/_topcoat-svelte/runtime/server/server.js").await,
        StatusCode::NOT_FOUND
    );

    let cx = CxTestBuilder::new().build();
    let cx = &cx;
    let html = view! { cx => (WIDGET.island(cx, &serde_json::json!({ "count": 7 }))) }
        .unwrap()
        .render(cx);
    assert!(html.contains("data-tcs-ssr"), "ssr did not render: {html}");
    assert!(html.contains("<button>count 7</button>"), "{html}");
}
