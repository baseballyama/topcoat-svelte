//! Server-side page tests, gated on the `ssr` feature. They compile a page
//! component through `svelte!`, render a full document with
//! [`SvelteComponent::page`], and assert the document is server-rendered: the
//! `<svelte:head>` content lands in `<head>`, and the body carries the hydration
//! root with the server markup.
#![cfg(feature = "ssr")]

use topcoat::context::CxTestBuilder;
use topcoat::view::view;
use topcoat_svelte::{SvelteComponent, svelte};

static DOC: SvelteComponent = svelte!("./fixtures/page/Doc.svelte");

#[tokio::test]
async fn page_document_is_server_rendered_with_head() {
    let cx = CxTestBuilder::new().build();
    let cx = &cx;
    let props = serde_json::json!({ "title": "Hello Rust", "count": 4 });
    let html = view! { cx => (DOC.page(cx, &props)) }.unwrap().render(cx);

    // A full document with the runtime wired into the head.
    assert!(html.starts_with("<!doctype html><html><head>"), "{html}");
    assert!(html.contains("type=\"importmap\""));
    assert!(html.contains("/_topcoat-svelte/loader.js?v="));

    // `<svelte:head>` content is lifted into the document head on the server.
    let head_end = html.find("</head>").unwrap();
    let title_at = html
        .find("<title>Hello Rust</title>")
        .unwrap_or_else(|| panic!("<svelte:head> title missing: {html}"));
    assert!(title_at < head_end, "title must be inside <head>: {html}");

    // The body is the hydration root, server-rendered and marked for hydration.
    assert!(html.contains("<body><"), "{html}");
    assert!(html.contains("data-tcs-ssr"));
    assert!(html.contains("<h1>Hello Rust</h1>"), "{html}");
    assert!(html.contains("count 4"), "{html}");
    // serde_json serializes object keys in sorted order.
    assert!(html.contains(
        "<script type=\"application/json\">{\"count\":4,\"title\":\"Hello Rust\"}</script>"
    ));
    assert!(html.trim_end().ends_with("</body></html>"), "{html}");
}

#[tokio::test]
async fn page_composes_rust_extra_head() {
    let cx = CxTestBuilder::new().build();
    let cx = &cx;
    let props = serde_json::json!({ "title": "Doc", "count": 0 });
    let extra = view! { cx => <meta name="description" content="a Svelte page"> }.unwrap();
    let html = view! { cx => (DOC.page(cx, &props).with_head(extra)) }
        .unwrap()
        .render(cx);

    let head_end = html.find("</head>").unwrap();
    // Both the component's <svelte:head> and the Rust-supplied head sit in <head>.
    assert!(html.find("<title>Doc</title>").unwrap() < head_end);
    assert!(
        html.find("name=\"description\"").unwrap() < head_end,
        "extra head must be inside <head>: {html}"
    );
}
