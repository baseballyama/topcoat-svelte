//! A minimal Topcoat app that mounts a Svelte 5 counter as a client-side island
//! with initial props from Rust.

use topcoat::{
    Result,
    context::Cx,
    router::{Router, RouterBuilderDiscoverExt, page},
    view::view,
};
use topcoat_svelte::{SvelteComponent, svelte};

static COUNTER: SvelteComponent = svelte!("./Counter.svelte");

async fn render_index(cx: &Cx) -> Result {
    view! { cx =>
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <title>"Rust + Svelte"</title>
                (topcoat_svelte::script())
            </head>
            <body>
                <h1>"Rust + Svelte island"</h1>
                <p>"The button below is a Svelte 5 component, mounted on the client."</p>
                (COUNTER.island(cx, &serde_json::json!({ "count": 3 })))
            </body>
        </html>
    }
}

#[page("/")]
async fn index(cx: &Cx) -> Result {
    render_index(cx).await
}

/// Builds the router: the app's pages plus the `topcoat-svelte` asset route.
pub fn router() -> Router {
    Router::builder()
        .route(topcoat_svelte::serve)
        .discover()
        .build()
}

#[tokio::main]
async fn main() {
    topcoat::start(router()).await.unwrap();
}

#[cfg(test)]
mod tests {
    use topcoat::context::CxTestBuilder;

    use super::*;

    #[tokio::test]
    async fn page_renders_counter_island() {
        let cx = CxTestBuilder::new().build();
        let html = render_index(&cx).await.unwrap().render(&cx);

        // The island marker, its module URL (with a content hash), and the
        // initial props are all present in the rendered HTML.
        assert!(html.contains("data-tcs-island"));
        assert!(html.contains("data-tcs-module=\"/_topcoat-svelte/c/Counter-"));
        assert!(html.contains("<script type=\"application/json\">{\"count\":3}</script>"));

        // The runtime is wired up: import map first, then the loader.
        assert!(html.contains("type=\"importmap\""));
        assert!(html.contains("/_topcoat-svelte/loader.js?v="));

        // `Counter.svelte` imports `Label.svelte`, so both are compiled and
        // registered, and the counter module references the label by URL.
        let names: Vec<&str> = topcoat_svelte::compiled_modules()
            .map(|m| m.name())
            .collect();
        assert!(names.contains(&"Counter"));
        assert!(names.contains(&"Label"));

        let counter = topcoat_svelte::compiled_modules()
            .find(|m| m.name() == "Counter")
            .unwrap();
        assert!(counter.js().contains("/_topcoat-svelte/c/Label-"));
        assert!(!counter.js().contains("'./Label.svelte'"));

        // With the `ssr` feature, the counter (and its Label child) are
        // server-rendered into the island for the client to hydrate.
        #[cfg(feature = "ssr")]
        {
            assert!(html.contains("data-tcs-ssr"));
            assert!(html.contains("clicked"), "server markup missing: {html}");
        }
    }
}
