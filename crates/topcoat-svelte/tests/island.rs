//! Compiles a fixture component through the `svelte!` macro and asserts the
//! rendered island HTML carries the marker, the hashed module URL, and the
//! escaped props.

use topcoat::context::CxTestBuilder;
use topcoat::view::view;
use topcoat_svelte::{SvelteComponent, svelte};

static COUNTER: SvelteComponent = svelte!("./fixtures/counter.svelte");

#[tokio::test]
async fn island_renders_marker_module_and_props() {
    let cx = CxTestBuilder::new().build();
    let cx = &cx;
    let html = view! { cx => (COUNTER.island(cx, &serde_json::json!({ "count": 7 }))) }
        .unwrap()
        .render(cx);

    assert!(html.contains("data-tcs-island"));
    assert!(html.contains("data-tcs-module=\"/_topcoat-svelte/c/Counter-"));
    assert!(html.contains("style=\"display:contents\""));
    assert!(html.contains("<script type=\"application/json\">{\"count\":7}</script>"));

    // The module URL carries a 16-hex-character content hash.
    let marker = "/_topcoat-svelte/c/Counter-";
    let start = html.find(marker).unwrap() + marker.len();
    let hash: String = html[start..].chars().take_while(|c| *c != '.').collect();
    assert_eq!(hash.len(), 16, "hash was {hash:?}");
    assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn island_escapes_script_breakout_in_props() {
    let cx = CxTestBuilder::new().build();
    let cx = &cx;
    let html = view! { cx =>
        (COUNTER.island(cx, &serde_json::json!({ "x": "</script><script>alert(1)</script>" })))
    }
    .unwrap()
    .render(cx);

    assert!(!html.contains("</script><script>alert"));
    assert!(html.contains("\\u003c/script\\u003e"));
}
