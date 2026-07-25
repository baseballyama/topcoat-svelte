# Svelte islands

`topcoat-svelte` lets you drop a [Svelte 5](https://svelte.dev) component into a
Topcoat page as a client-rendered **island**. The component is compiled to
JavaScript at Rust build time by [rsvelte](https://github.com/baseballyama/rsvelte),
so your app needs only the Rust toolchain -- no Node.js, no npm, no bundler.

By default, islands render empty on the server and mount on the client. Enable
the [`ssr` feature](#server-side-rendering-the-ssr-feature) to server-render each
island's HTML and hydrate it in the browser.

## The three pieces

Using a Svelte component takes three things:

1. `svelte!("./Component.svelte")` -- compiles the component and gives you a
   `SvelteComponent` handle.
2. `component.island(cx, &props)` -- renders the component as a node inside
   `view!`, seeded with props from Rust.
3. `topcoat_svelte::script()` and `topcoat_svelte::serve` -- the runtime plumbing
   that mounts islands in the browser and serves the JavaScript.

## A complete example

`Counter.svelte`:

```svelte
<script>
	let { count = 0 } = $props();
	let n = $state(count);
</script>

<button onclick={() => (n += 1)}>clicked {n} times</button>
```

`main.rs`:

```rust,ignore
use topcoat::{
    Result,
    context::Cx,
    router::{Router, RouterBuilderDiscoverExt, page},
    view::view,
};
use topcoat_svelte::{SvelteComponent, svelte};

static COUNTER: SvelteComponent = svelte!("./Counter.svelte");

#[page("/")]
async fn index(cx: &Cx) -> Result {
    view! {
        <!DOCTYPE html>
        <html>
            <head>
                <meta charset="utf-8">
                (topcoat_svelte::script())
            </head>
            <body>
                (COUNTER.island(cx, &serde_json::json!({ "count": 3 })))
            </body>
        </html>
    }
}

pub fn router() -> Router {
    Router::builder()
        .route(topcoat_svelte::serve)
        .discover()
        .build()
}
```

## `svelte!`

`svelte!("./Counter.svelte")` reads and compiles the component when your crate
compiles, and evaluates to a `SvelteComponent`. Assign it to a `static` (or
`const`) so the work happens once:

```rust,ignore
static COUNTER: SvelteComponent = svelte!("./Counter.svelte");
```

Path resolution mirrors `asset!`:

| Path | Resolved relative to |
|---|---|
| `"./Counter.svelte"`, `"../ui/Counter.svelte"` | the calling source file |
| `"components/Counter.svelte"` | the crate's `CARGO_MANIFEST_DIR` |
| `"/abs/Counter.svelte"` | used as-is |

The component name is the PascalCase file stem (`counter.svelte` and
`my-widget.svelte` become `Counter` and `MyWidget`). The derived name must be
ASCII -- a non-ASCII file stem (e.g. `café.svelte`) produces a `compile_error!`
asking you to rename the file, since a non-ASCII `module_url` would not match
the percent-encoded path browsers actually request, causing the component's
module to 404. Editing the `.svelte` file triggers a rebuild. A Svelte compile
error becomes a Rust `compile_error!` that points at the `svelte!` call.

### Importing other components

A component may import other `.svelte` files by relative path, and the whole
reachable graph is compiled and served for you:

```svelte
<script>
	import Label from './Label.svelte';
	import Panel from '../ui/Panel.svelte';
	let { count = 0 } = $props();
</script>

<Panel>
	<Label text="clicks" /> {count}
</Panel>
```

You only write `svelte!("./Counter.svelte")` for the entry component; every
`.svelte` file it imports (and everything those import, transitively) is
compiled, registered, and served under its own content-hashed URL. Imports are
rewritten to those URLs, so a component's hash changes whenever any component it
depends on changes -- caches never serve a stale child. Editing any file in the
graph rebuilds the affected components.

Limits:

- Only **relative** specifiers ending in `.svelte` are resolved (`./Child.svelte`,
  `../ui/Panel.svelte`). Bare or npm package imports
  (`import X from 'some-lib/Widget.svelte'`) are not supported -- there is no
  local file to compile.
- Two files may share a name (`a/Button.svelte` and `b/Button.svelte`) in one
  graph without colliding; their differing content gives them distinct URLs.
- A cycle (`A.svelte` imports `B.svelte`, which imports `A.svelte`) is a
  compile error.

## `island`

`component.island(cx, &props)` returns a value you place in node position inside
`view!`. It renders a small placeholder:

```html
<div data-tcs-island data-tcs-module="/_topcoat-svelte/c/Counter-a1b2c3d4e5f60718.js"
     style="display:contents">
  <script type="application/json">{"count":3}</script>
</div>
```

`props` is any `serde::Serialize` value; a `serde_json::json!({ ... })` object is
the common choice. The props become the component's `$props()`. The JSON is
escaped so a string value can never break out of the `<script>` element. The
`display:contents` wrapper keeps the island from affecting layout. `cx` is
currently unused and reserved for future server-side rendering.

## `script` and `serve`

`topcoat_svelte::script()` emits two tags, and belongs in `<head>`:

- an **import map** pointing the `svelte*` module specifiers at the vendored
  runtime, which must appear before any module script, and
- the **island loader**, which finds every island on the page, imports its
  module, and mounts the component with its props.

`topcoat_svelte::serve` is the route that serves everything under
`/_topcoat-svelte/`: the vendored Svelte runtime, the loader, and each compiled
component module -- all from memory, with immutable caching (every URL is
content-hashed). Register it with `.route(topcoat_svelte::serve)`.

## How it fits together

At build time, `svelte!` compiles each component to a JavaScript module and
registers it in the binary. At request time, `island` renders a placeholder that
names the module's content-hashed URL, and `script()` adds the import map and
loader. In the browser, the loader imports each island's module and mounts it,
resolving every `svelte*` import through the import map to the vendored runtime
that `serve` provides.

## Server-side rendering (the `ssr` feature)

By default islands are client-rendered. Enable the `ssr` feature to
server-render each island's HTML and hydrate it in the browser, so the content
is present in the initial response:

```toml
[dependencies]
topcoat-svelte = { version = "0.1", features = ["ssr"] }
```

Nothing else in your code changes: the same `island(cx, &props)` call now emits
the component's server-rendered markup inside the island (marked `data-tcs-ssr`),
and the loader hydrates it instead of mounting from scratch. The props embedded
for the client are the exact same JSON used for the server render, so the two
sides always agree.

How it works: `svelte!` compiles every component to server JavaScript as well as
client JavaScript. When the feature is on, `island` runs that server code in an
embedded QuickJS engine (one per thread, reused across renders) to produce the
HTML. A component's imports resolve in the engine exactly as they do in the
browser, so a whole module graph server-renders correctly.

Notes and limits:

- **Build cost.** The feature pulls in `rquickjs`, which builds QuickJS from C,
  so an `ssr` build needs a C compiler. A default (client-only) build does not.
- **Graceful fallback.** If a server render fails, that island silently falls
  back to client rendering (an empty island the browser mounts) rather than
  failing the response.
- **Unstyled flash.** Component CSS is still injected by JavaScript at hydration
  time, so server-rendered markup can appear briefly unstyled until the island
  hydrates. Serving extracted CSS is a future improvement.
