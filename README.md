# topcoat-svelte

Use Svelte 5 components inside [Topcoat](https://crates.io/crates/topcoat) apps —
compiled by [rsvelte](https://github.com/baseballyama/rsvelte), so the app
developer needs only the Rust toolchain (no Node.js).

**Status: experimental (Phase 1 — client-rendered islands, no SSR yet).**

```rust
use topcoat::{Result, context::Cx, router::page, view::view};
use topcoat_svelte::{SvelteComponent, svelte};

static COUNTER: SvelteComponent = svelte!("./Counter.svelte");

#[page("/")]
async fn index(cx: &Cx) -> Result {
    view! {
        <html>
            <head>(topcoat_svelte::script())</head>
            <body>
                <h1>"Rust + Svelte"</h1>
                (COUNTER.island(cx, &serde_json::json!({ "count": 3 })))
            </body>
        </html>
    }
}
```

Register the asset route with `.route(topcoat_svelte::serve)` when building your
router.

See [`crates/topcoat-svelte/docs/islands.md`](crates/topcoat-svelte/docs/islands.md)
for the full guide, [`DESIGN.md`](DESIGN.md) for the architecture and roadmap, and
[`examples/counter`](examples/counter) for a runnable app.

## Development

The `runtime/` directory contains the maintainer-only JS build that vendors the
Svelte client runtime into `crates/topcoat-svelte/runtime/dist/` (committed).
Regenerate with `cd runtime && pnpm install && node build.mjs` after bumping the
pinned Svelte version. Application builds never require Node.
