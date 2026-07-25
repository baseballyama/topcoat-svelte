//! Client-navigation data-protocol tests. A page route answers a
//! `X-Topcoat-Svelte: data` request with the JSON contract (module URL + props)
//! and a normal request with the HTML document; both declare
//! `Vary: X-Topcoat-Svelte`. Runs in both feature modes (the data reply skips
//! server rendering; the document reply server-renders under `ssr`).

use http::header::{CONTENT_TYPE, VARY};
use http::request::Parts;
use topcoat::context::CxTestBuilder;
use topcoat::view::view;
use topcoat_svelte::{SvelteComponent, svelte};

static DOC: SvelteComponent = svelte!("./fixtures/page/Doc.svelte");

/// A GET request for `/page`, optionally carrying the data header.
fn request(data: bool) -> Parts {
    let mut builder = http::Request::builder().method("GET").uri("/page");
    if data {
        builder = builder.header("X-Topcoat-Svelte", "data");
    }
    builder.body(()).unwrap().into_parts().0
}

/// Extracts and parses the props embedded in a page document's hydration root.
fn embedded_props(html: &str) -> serde_json::Value {
    let marker = "<script type=\"application/json\">";
    let start = html.find(marker).expect("props script") + marker.len();
    let end = html[start..].find("</script>").expect("props script end") + start;
    serde_json::from_str(&html[start..end]).expect("valid props json")
}

#[tokio::test]
async fn data_request_returns_json_contract() {
    let cx = CxTestBuilder::new().request_context(request(true)).build();
    let cx = &cx;
    let props = serde_json::json!({ "title": "Doc", "count": 7 });
    let rendered = view! { cx => (DOC.page(cx, &props)) }
        .unwrap()
        .render_response(cx);

    // The body is the JSON contract: the component's served module URL and the
    // props value, not an HTML document. (Parsed rather than string-matched so
    // the assertions do not depend on JSON object key order.)
    assert!(
        !rendered.html.contains("<!doctype html>"),
        "{}",
        rendered.html
    );
    let body: serde_json::Value = serde_json::from_str(&rendered.html).expect("json data body");
    assert!(
        body["module"]
            .as_str()
            .unwrap()
            .starts_with("/_topcoat-svelte/c/Doc-"),
        "{}",
        rendered.html
    );
    assert_eq!(body["props"], props);

    // JSON content type (replacing the view's text/html default) and Vary.
    assert_eq!(
        rendered.headers.get(CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(rendered.headers.get(VARY).unwrap(), "X-Topcoat-Svelte");
}

#[tokio::test]
async fn normal_request_returns_html_document() {
    let cx = CxTestBuilder::new().request_context(request(false)).build();
    let cx = &cx;
    let props = serde_json::json!({ "title": "Doc", "count": 7 });
    let rendered = view! { cx => (DOC.page(cx, &props)) }
        .unwrap()
        .render_response(cx);

    assert!(
        rendered.html.starts_with("<!doctype html><html><head>"),
        "{}",
        rendered.html
    );
    // The page hydration root carries the client-router marker.
    assert!(rendered.html.contains("data-tcs-page"));

    // The document declares Vary but no JSON content type: text/html is the
    // router default, applied when the view becomes a response.
    assert_eq!(rendered.headers.get(VARY).unwrap(), "X-Topcoat-Svelte");
    assert!(rendered.headers.get(CONTENT_TYPE).is_none());

    // Props identity: the embedded script carries the same props value the data
    // reply splices into its JSON body.
    assert_eq!(embedded_props(&rendered.html), props);
}
