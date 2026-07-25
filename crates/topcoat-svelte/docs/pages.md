# Svelte pages

An [island](islands.md) is a Svelte component embedded inside a Topcoat `view!`.
A **page** is the other way around: the whole HTML document *is* one Svelte
component tree, and the Rust `#[page]` function plays the role of SvelteKit's
`load()` -- it computes props (from the database, auth, the request `Cx`) and
hands them to a page-level `.svelte` component.

`component.page(cx, &props)` renders a full `<!doctype html>` document:

- the `<head>` carries `topcoat_svelte::script()`'s import map and loader, plus
  the component's `<svelte:head>` content when server-rendered, and
- the `<body>` is a single hydration root spanning the whole page.

It reuses the island hydration machinery, so with the `ssr` feature the document
arrives server-rendered and hydrates with no extra client code.

## A complete example

`Page.svelte` -- a page composing smaller components through ordinary imports:

```svelte
<script>
	import Counter from './Counter.svelte';
	let { count = 0 } = $props();
</script>

<svelte:head>
	<title>Rust + Svelte page</title>
</svelte:head>

<main>
	<h1>Full Svelte page</h1>
	<Counter {count} />
</main>
```

`main.rs`:

```rust,ignore
use topcoat::{Result, context::Cx, router::page, view::view};
use topcoat_svelte::{SvelteComponent, svelte};

static PAGE: SvelteComponent = svelte!("./Page.svelte");

#[page("/page")]
async fn page_route(cx: &Cx) -> Result {
    // `#[page]` is the `load()`: compute props in Rust, hand them to Svelte.
    view! { cx => (PAGE.page(cx, &serde_json::json!({ "count": 5 }))) }
}
```

Register `topcoat_svelte::serve` on the router as usual. Note there is no
`<html>`/`<head>`/`<body>` scaffolding and no `topcoat_svelte::script()` call in
Rust: `page` emits the whole document, including the runtime plumbing.

## `page`

`component.page(cx, &props)` returns a `Page` value that you place in node
position inside `view!` (typically as the entire body of a `#[page]` function).
It emits, in order:

```html
<!doctype html><html><head>
  <!-- import map + loader (from script()) -->
  <!-- the component's <svelte:head> content, when server-rendered -->
</head><body>
  <div data-tcs-island data-tcs-ssr data-tcs-module="/_topcoat-svelte/c/Page-<hash>.js"
       style="display:contents">
    <!-- server-rendered body, when the ssr feature is on -->
    <script type="application/json">{"count":5}</script>
  </div>
</body></html>
```

The body root is exactly the same shape as an island (`data-tcs-island` +
`data-tcs-module` + embedded props, plus `data-tcs-ssr` when server-rendered),
so the existing loader hydrates a page with zero page-specific client code.
`props` is any `serde::Serialize` value and becomes the page component's
`$props()`, escaped so it can never break out of the `<script>` element.

### Adding head content from Rust

`<svelte:head>` handles head content the component owns. For head content Rust
computes (a per-request description, canonical URL, Open Graph tags), chain
`Page::with_head`, passing a `view!` (unwrap its `Result` with `?`):

```rust,ignore
#[page("/page")]
async fn page_route(cx: &Cx) -> Result {
    let description = compute_description(cx);
    view! { cx =>
        (PAGE.page(cx, &serde_json::json!({ "count": 5 })).with_head(view! { cx =>
            <meta name="description" content=(description)>
        }?))
    }
}
```

Rust-supplied head composes *after* `script()`'s output and the component's
`<svelte:head>`, all inside `<head>`.

## Feature interplay: SSR and CSR

`page` works with and without the `ssr` feature, following the same props path:

- **With `ssr`** -- the document arrives server-rendered. The component's
  `<svelte:head>` content lands in the document head, the body root holds the
  server-rendered markup (marked `data-tcs-ssr`), and the client hydrates.
- **Without `ssr`** -- the head has no `<svelte:head>` output and the body root
  is empty; the browser mounts the page component on the client (SPA-style),
  and Svelte applies `<svelte:head>` at mount as usual.

Enabling SSR is a Cargo feature flip; no code changes. See
[the islands guide](islands.md#server-side-rendering-the-ssr-feature) for the
build cost, graceful-fallback, and unstyled-flash notes -- they apply to pages
identically (a page render that throws degrades to the empty client-mounted
document rather than failing the response).

## Layouts

Pages need no new layout machinery, and the two layout styles compose:

- **Svelte-side layouts** are ordinary component composition. A layout component
  takes a `children` snippet and renders it with `{@render children()}`; the
  page imports and wraps its content in that layout through the normal module
  graph. Nothing here is Topcoat-specific.
- **Rust-side layouts** (Topcoat `#[layout]`) still apply, with one rule:
  because `page` already emits the complete `<!doctype html>` document, a
  `#[layout]` around a Svelte page must not add markup of its own -- use it for
  cross-cutting non-markup work (auth checks, redirects, response headers).
  Layouts that render a document shell belong with `view!` pages and islands,
  where the shell is yours to write.

## Client-side navigation (Inertia-style)

Once a document is a Svelte page, the loader turns same-origin link clicks into
**soft navigations**: no full reload, just a swap of the page component. This is
the [Inertia.js](https://inertiajs.com) model -- server-driven routing with a
client-side component swap -- not a ported SvelteKit router. It is automatic;
there is no client API to call.

### The data protocol

`page` answers a request in one of two ways, decided from the request headers:

- a normal request gets the HTML document (as above);
- a request carrying `X-Topcoat-Svelte: data` gets JSON instead:
  `{"module": "/_topcoat-svelte/c/<Name>-<hash>.js", "props": <the props value>}`,
  with `Content-Type: application/json`.

The **same `#[page]` function** serves both -- it *is* the `load()`; the `Page`
node inspects the request and shapes the response. Both replies carry
`Vary: X-Topcoat-Svelte` so a cache never hands an HTML document to a data
request or the reverse. The `props` in the JSON is byte-for-byte the same string
embedded in the initial document, so the value the client mounts with on
navigation is identical to the one it would hydrate with on a fresh load.

### What the loader does

- **Intercepts** clicks on same-origin `<a>` elements -- unless the click has a
  modifier key, or the link has `target`, `download`, `data-tcs-reload`, or is a
  pure-hash link on the current page.
- **Fetches** the target with `X-Topcoat-Svelte: data`, expecting JSON. Anything
  else -- a network error, a non-page route, a redirect to another content type
  -- **falls back to a full navigation** (`location.assign`). Without JavaScript
  the links are ordinary links, so navigation degrades cleanly. If you already
  know a link points at a non-page route (a download, an external redirect,
  etc.), mark it `data-tcs-reload` to skip this probe fetch and navigate
  directly.
- **Swaps** the page: dynamically imports the new module, unmounts the current
  page component, and mounts the new one into the same hydration root with the
  new props (a client render -- hydration is only for the first document). Then
  it `pushState`s the URL, scrolls to the top (or to a `#fragment` target), and
  Svelte applies the new `<svelte:head>` (title etc.).
- **Back/forward** (`popstate`) re-fetches and swaps for the restored URL.
- **Prefetches** on `pointerover`/`focus`: the data fetch and module import run
  ahead of the click, cached per URL for the document's lifetime. Prefetch
  failures are silent; the click path retries or falls back.

Opt a single link out of interception with `data-tcs-reload` (it will do a full
navigation):

```html
<a href="/report.pdf" data-tcs-reload>Download</a>
```

The router only activates when the document has a page hydration root
(`data-tcs-page`); island-only documents get no interception.

### State semantics (v1)

The **whole** page component tree is swapped on navigation. Svelte-side layout
component state does **not** survive a navigation yet (SvelteKit preserves layout
instances across route changes; this does not). Islands mounted outside the page
root are untouched by navigation. These limits are expected to lift in a later
stage.

## Islands vs. pages

| | Island | Page |
|---|---|---|
| What it is | a component inside a Topcoat `view!` | the whole document is one component |
| Rust call | `component.island(cx, &props)` | `component.page(cx, &props)` |
| Document shell | you write `<html>`/`<head>`/`<body>` in `view!` | `page` emits the whole document |
| `script()` | you place it in `<head>` yourself | included automatically |
| `<svelte:head>` | applied on hydrate/mount | lifted into `<head>` when server-rendered |
| Feel | Next.js-style islands in a Rust-rendered page | SvelteKit-style page with a Rust `load()` |

Both share the same runtime, loader, and `serve` route; mix them freely in one
app.
