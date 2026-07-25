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
