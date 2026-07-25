# topcoat-svelte

Use Svelte 5 components inside [Topcoat](https://crates.io/crates/topcoat) apps —
compiled by [rsvelte](https://github.com/baseballyama/rsvelte), so the app
developer needs only the Rust toolchain (no Node.js).

**Status: experimental.** Working today:

- **Islands** — embed a Svelte component in a Topcoat `view!` page
  (`component.island(cx, &props)`).
- **Pages** — render a whole HTML document from one Svelte component tree, with
  the `#[page]` function as SvelteKit's `load()` (`component.page(cx, &props)`).
- **Module graph** — a component may `import` other `.svelte` files by relative
  path; the whole reachable graph is compiled and served.
- **SSR + hydration** — opt in with the `ssr` feature to server-render islands
  and pages and hydrate them in the browser; off by default (a client-only build
  needs no C compiler).

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
and [`crates/topcoat-svelte/docs/pages.md`](crates/topcoat-svelte/docs/pages.md)
for the full guides, [`DESIGN.md`](DESIGN.md) for the architecture and roadmap,
and [`examples/counter`](examples/counter) for a runnable app (islands at `/`, a
full Svelte page at `/page`).

## Building

[rsvelte](https://github.com/baseballyama/rsvelte) is not on crates.io yet, so
this workspace expects a sibling checkout at `../rsvelte`:

```sh
git clone https://github.com/baseballyama/rsvelte ../rsvelte
```

(A `git` dependency is not usable today: cargo fetches git submodules of git
dependencies recursively, and rsvelte's test-fixture submodules are huge,
partly SSH-only, and currently fail to resolve. This becomes a normal
crates.io dependency once rsvelte_core is published.)

## Development

The `runtime/` directory contains the maintainer-only JS build that vendors the
Svelte client runtime into `crates/topcoat-svelte/runtime/dist/` (committed).
Regenerate with `cd runtime && pnpm install && node build.mjs` after bumping the
pinned Svelte version. Application builds never require Node.
