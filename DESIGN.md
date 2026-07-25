# topcoat-svelte — Design (Phase 1: islands, client-side rendering only)

Integrate Svelte 5 components into [Topcoat](https://crates.io/crates/topcoat) apps
using [rsvelte](https://github.com/baseballyama/rsvelte) as the compiler, so that
application developers need **only the Rust toolchain** — no Node.js, no npm.

## Goals (Phase 1)

- `svelte!("./Counter.svelte")` compiles a Svelte 5 component at Rust compile time
  via `rsvelte_core` and makes it embeddable inside Topcoat's `view!` as a
  client-rendered **island**.
- Props are passed from Rust (any `serde::Serialize` value) to the component.
- Compiled JS modules and the vendored Svelte client runtime are served from
  memory by the Topcoat app itself — no files written to disk, no bundler run by
  the app developer.
- The Svelte islands coexist with Topcoat's own runtime (`topcoat::dev::script()`).
- A component may `import` other `.svelte` files by **relative path**
  (`./Child.svelte`, `../ui/Panel.svelte`). The whole reachable graph is compiled
  and served; see "Module graph" below.

## Non-goals (Phase 1)

- No SSR / hydration. Islands render empty on the server and mount on the client.
  (Phase 2 will explore executing rsvelte's server output in an embedded JS engine.)
- No typed props derived from the component's `<script>` (props are `impl Serialize`;
  typing may later come from rsvelte's svelte2tsx port).
- No Topcoat `topcoat asset` CLI integration; this crate has its own serving route.

## Why not `topcoat-asset`

`asset!` embeds a *record pointing at a file on disk* (crate name + manifest dir +
path); the bundler later reads the file from the filesystem. Macro-generated JS has
no on-disk file, so instead we serve compiled modules **from memory** through a
dedicated route, with our own content hashing for immutable caching.

## Repository layout

```
topcoat-svelte/
  Cargo.toml                     # workspace
  crates/
    topcoat-svelte/              # runtime crate (the one apps depend on)
      src/
      runtime/dist/              # vendored, committed JS (svelte runtime + loader)
    topcoat-svelte-macro/        # proc-macro crate: svelte!
  runtime/                       # maintainer-only JS build for runtime/dist
    package.json                 # pins svelte 5.56.4 + esbuild
    build.mjs
    loader.js                    # island loader source
  examples/
    counter/
```

## Dependencies

- `topcoat` / `topcoat-view` / `topcoat-router`: **crates.io `0.4`**. Public API only.
- `rsvelte_core`: **path dependency** `../rsvelte/crates/rsvelte_core` (relative to
  the workspace root's parent, i.e. `~/git/rsvelte`). Not yet on crates.io; switch
  when published. Only the proc-macro crate depends on it.
- `inventory` for distributed registration of compiled modules.
- `serde` / `serde_json` for props.
- This repo's own code is `#![forbid(unsafe_code)]` (dependencies may use unsafe).

## The contract (pinned so independent work can integrate)

### URL namespace

All JS is served under `/_topcoat-svelte/{file}`:

- Vendored runtime files keep their committed names: e.g.
  `/_topcoat-svelte/runtime/client.js` (see "Vendored runtime" for the exact set).
  Cache-busting for runtime files uses a query param `?v={runtime_hash}` where
  `runtime_hash` is a compile-time FNV/SHA hash of the vendored bytes.
- Compiled components: `/_topcoat-svelte/c/{ComponentName}-{hash}.js` where
  `{hash}` is a compile-time hash (first 16 hex chars) of the compiled client JS.

All responses: `Content-Type: text/javascript; charset=utf-8`,
`Cache-Control: public, max-age=31536000, immutable`.

### HTML shape of an island

```html
<div data-tcs-island data-tcs-module="/_topcoat-svelte/c/Counter-abc123.js"
     style="display:contents">
  <script type="application/json">{"count":0}</script>
</div>
```

- The wrapper div uses `display:contents` so it doesn't affect layout.
- Props JSON goes through Topcoat's normal escaping path where possible; the JSON
  script element must escape `</script` (serde_json string + replace `<` with `<`
  during serialization — use `serde_json::to_string` then a safe substitution, or a
  serializer wrapper).

### `topcoat_svelte::script()` component

A Topcoat `#[component]` (placed in `<head>` or end of `<body>` by the app) that emits:

1. `<script type="importmap">` mapping bare specifiers used by rsvelte/Svelte
   compiled output to the served runtime URLs. Minimum set (verify against actual
   compiled output during implementation):
   - `svelte` (for `mount`)
   - `svelte/internal/client`
   - `svelte/internal/disclose-version`
   - `svelte/internal/flags/legacy` (only if legacy-mode output is supported; if
     bundling it is awkward, restrict Phase 1 to runes mode and document it)
2. `<script type="module" src="/_topcoat-svelte/loader.js?v={hash}">` — the loader.

Note: the importmap must appear **before** any module scripts; document that
`script()` belongs in `<head>`.

### Loader behavior (`runtime/loader.js`)

- On `DOMContentLoaded` (or immediately if already loaded), query
  `[data-tcs-island]`, for each: read `data-tcs-module`, parse the child JSON
  script's text as props, `const mod = await import(moduleUrl)`, then
  `mount(mod.default, { target: el, props })` with `mount` imported from `svelte`.
- Islands added later are out of scope (no MutationObserver in Phase 1).
- Errors: `console.error` per island; one failing island must not break others.

### Rust API (crate `topcoat-svelte`)

```rust
// App code:
use topcoat_svelte::{svelte, SvelteComponent};

static COUNTER: SvelteComponent = svelte!("./Counter.svelte");

#[page]
async fn index(cx: &Cx) -> Result<View> {
    Ok(view! {
        <head>(topcoat_svelte::script())</head>
        <body>(COUNTER.island(cx, &serde_json::json!({ "count": 3 })))</body>
    })
}

// Router setup:
router.route(topcoat_svelte::serve) // the /_topcoat-svelte/{file} route
```

- `SvelteComponent` is a small const-constructible handle (name, hash, module URL).
- `island(&self, props: &impl Serialize) -> Island` where `Island` implements the
  Topcoat view traits (`NodeViewParts` — check exact trait paths in topcoat-view 0.4)
  so it can sit in node position inside `view!`. If `NodeViewParts` isn't public in
  0.4, fall back to returning a `View` via `Unescaped` (public alternative:
  `island_view()` -> `View`). Prefer whichever is public and idiomatic in 0.4.
- `serve`: a `#[route]`-style handler (or whatever Topcoat 0.4's manual route
  registration expects) resolving files from the in-memory registry.

### `svelte!` macro (crate `topcoat-svelte-macro`)

- Input: string literal path, resolved **relative to the call-site source file's
  directory** (mirror `asset!("./…")` semantics; also accept crate-root-relative
  without `./` if easy, else document).
- Reads the `.svelte` source at macro-expansion time; ALSO emits
  `const _: &str = include_str!("…");` with the same absolute path so rustc
  re-expands when the file changes.
- Calls `rsvelte_core::compiler::compile(source, options)` with:
  - `generate: Client`
  - `css: Injected` (styles injected by JS at mount; no separate CSS pipeline in Phase 1)
  - component name derived from the file stem (PascalCase as Svelte convention).
- On compile error: emit `compile_error!` with the rsvelte diagnostic (message +
  line/col if available).
- Expansion registers the module via `inventory::submit!` with
  `{ name, hash, js: &'static str }` and evaluates to a `SvelteComponent` const/static.
- Hashing at macro time (e.g. SHA-256 via `sha2`, truncated) so URLs are stable
  per content.

### Vendored runtime (`runtime/`, maintainer-only)

- `package.json` pins `svelte@5.56.4` (must match the Svelte version rsvelte
  targets) and `esbuild`.
- `build.mjs` bundles these entry points as ESM with code splitting into
  `crates/topcoat-svelte/runtime/dist/`:
  - `svelte` → `runtime/svelte.js`
  - `svelte/internal/client` → `runtime/client.js`
  - `svelte/internal/disclose-version` → `runtime/disclose-version.js`
  - (plus `flags/legacy` if kept in scope)
  - and copies/minifies `loader.js` → `loader.js`
- Chunk files created by splitting live in the same dir; the serve route serves the
  whole embedded dir, so relative chunk imports resolve. Embed the dist dir into the
  crate with `include_str!`/a small `build.rs`-generated table, or the `include_dir`
  crate — implementer's choice, but no runtime filesystem access.
- The dist output is **committed** so `cargo build` never needs Node.
- Sanity requirement: importmap keys must cover every bare specifier appearing in
  rsvelte's compiled client output for the example components (grep the compiled JS
  in a test).

## Testing

1. Unit test in `topcoat-svelte-macro` (or an integration test in `topcoat-svelte`):
   compile a fixture `.svelte` through the macro; assert the rendered island HTML
   contains the marker div, module URL with hash, and escaped props JSON.
2. A test that renders a full page `View` (with `script()`) and asserts the
   importmap covers every `from "…"` / `import "…"` bare specifier found in the
   registered modules' JS.
3. `examples/counter` builds (`cargo build -p counter-example`) and its page test
   asserts HTML output. Manual browser verification happens after review.
4. `cargo test --workspace` green, `cargo clippy --workspace` clean, `cargo fmt` clean.

## Module graph

A component's `<script>` may import other components by relative path
(`import Child from './Child.svelte'`, also `../`). rsvelte preserves those
imports in its compiled client output, so after compiling a component the macro:

1. Parses the emitted JS with oxc and collects the string literals that are the
   *source* of a static `import` / `export ... from` declaration and end in
   `.svelte` with a relative (`./` or `../`) prefix. Parsing (rather than a text
   search) means a `.svelte` substring inside ordinary code -- a variable value,
   a text node -- is never mistaken for an import.
2. Resolves each specifier against the **importing file's** directory, then
   compiles that child the same way, depth-first. A child is compiled before the
   parent that imports it. A file reached by several routes (a diamond) is
   compiled once; a cycle is a compile error (its hash cannot be resolved, since
   each module's hash would depend on the other's URL).
3. Rewrites each specifier in the parent's JS to the child's served URL
   (`/_topcoat-svelte/c/{Child}-{hash}.js`) and only *then* hashes the parent, so
   the parent's hash folds in every child's hash: changing a grandchild changes
   every ancestor's URL (transitive cache busting).
4. Registers every module in the graph via `inventory::submit!` and emits an
   `include_str!` for every file, so editing any file in the graph triggers a
   recompile.

Two different files that share a stem (`a/Button.svelte`, `b/Button.svelte`)
used from one entry do not collide: their content differs, so their hashes and
therefore their served filenames (`Button-<hashA>.js` vs `Button-<hashB>.js`)
differ.

**Limits.** Only relative `.svelte` specifiers are resolved. Bare/npm package
imports (`import X from 'some-lib/Widget.svelte'`) remain unsupported -- there is
no filesystem location to compile them from. Dynamic `import('./X.svelte')` is
not rewritten -- rsvelte never emits it for child components, but user-written
code in a `<script>` block may contain one; it fails at run time with a clean
404 (the specifier resolves outside the registry), never with silent corruption.

## Phase 2: SSR + hydration (the `ssr` cargo feature)

Spike evidence: `docs/phase2-ssr-spike.md`. Pinned contract:

- **Feature.** `ssr` on `topcoat-svelte`, off by default. With it enabled,
  `island()` renders the component's server HTML into the island div and the
  client hydrates instead of mounting. Without it, behavior is exactly Phase 1.
- **Macro.** `svelte!` always uses `rsvelte_core::compile_both` and embeds BOTH
  client and server JS (one shared parse/analyze; the server text is dead weight
  only when `ssr` is off — acceptable). Server JS gets the same specifier
  rewriting as client JS; the rewritten URL doubles as the in-engine module key,
  so the module graph resolves identically on both sides.
- **Engine.** A minimal internal trait (e.g. `SsrEngine { render(module_key,
  props_json) -> Result<String> }`) with the rquickjs implementation behind the
  feature. One engine per thread (`thread_local!`), the server-runtime modules
  registered in a custom rquickjs resolver/loader: bare `svelte/server` /
  `svelte/internal/server` specifiers resolve to a NEW vendored server-runtime
  ESM bundle (built by `runtime/build.mjs` like the client one, committed under
  `runtime/dist/`), component keys resolve from the registry. Renders are
  synchronous and ~0.02 ms warm; no per-request engine setup.
- **Props identity.** The props JSON string embedded in the island's
  `<script type="application/json">` is THE input: the same string is
  `JSON.parse`d inside the engine for the server render and by the loader for
  hydration, guaranteeing the identical-props precondition for hydration.
- **Island HTML.** SSR islands additionally carry `data-tcs-ssr` and contain
  the server-rendered HTML (with Svelte's `<!--[-->`/`<!--]-->` markers) before
  the props script element. The loader branches: `data-tcs-ssr` → `hydrate()`,
  else `mount()`. The vendored `runtime/svelte.js` entry re-exports `hydrate`
  (no new importmap entries needed).
- **Failure mode.** If the server render throws, `island()` logs via the
  crate's established `eprintln!` convention and falls back to the Phase 1
  empty-div CSR island — an SSR bug degrades to client rendering, never a 500.
- **Known limit.** CSS stays `Injected` (applied at hydrate time), so SSR HTML
  can flash unstyled until hydration; extracted-CSS serving remains the fix and
  stays deferred.

## Stage 3: Svelte pages

The whole page is one Svelte component tree; the `#[page]` Rust fn plays
SvelteKit's `load()`. Pinned contract:

- **API.** `SvelteComponent::page(cx, &props) -> …` renders a **full HTML
  document**: `<!doctype html><html><head>` containing `script()`'s output
  (importmap + loader) plus the component's `<svelte:head>` content, then a
  `<body>` whose sole child is the hydration root. An optional builder-style
  hook lets Rust add extra head content (e.g.
  `.page_with_head(cx, &props, head_view)` or equivalent — implementer's
  choice, but Rust-supplied head must compose with `<svelte:head>` output).
- **Hydration root.** Same shape as an island (`data-tcs-island` +
  `data-tcs-module` + embedded props JSON + `data-tcs-ssr` when
  server-rendered) so the existing loader hydrates pages with zero new
  client code, with one addition: the root div of a page spans the body.
- **Head extraction.** The engine harness returns Svelte's
  `render()` result as `{ head, body }` (JSON) instead of body-only; the
  `SsrEngine` trait's render output becomes a small struct. `<svelte:head>`
  content lands in the document head on the server; on CSR-only builds it is
  applied by Svelte at mount as usual.
- **Feature interplay.** `page()` works with and without the `ssr` feature:
  with it, the document arrives server-rendered and hydrates; without it, the
  body root is empty and mounts on the client (SPA-style degradation, same
  props path). No hard dependency on `ssr`.
- **Layouts.** No new machinery: Svelte-side layouts are plain component
  composition (`{@render children()}` via the module graph); Rust-side
  `#[layout]`s keep working because a page's View is still a View — the
  hydration root just sits inside whatever shell the layout renders.
  Both styles are documented.
- **Escaping.** Server-rendered head/body strings enter the View through the
  unescaped path exactly like island SSR HTML; props keep the Phase 1
  escaping contract.

## Stage 4: client-side navigation (Inertia-style)

Server-driven routing with client-side page-component swaps. Pinned contract:

- **Data protocol.** A request carrying the header `X-Topcoat-Svelte: data`
  to a route whose handler renders a `page()` gets, instead of the HTML
  document, a JSON response (`Content-Type: application/json`,
  `Vary: X-Topcoat-Svelte`):
  `{"module": "/_topcoat-svelte/c/<Name>-<hash>.js", "props": <the props value>}`.
  The `Page` node itself answers this by inspecting the request `Cx` at render
  time — the `#[page]` fn stays byte-identical for both kinds of request (it IS
  the load()). Headers/content-type are set through topcoat's response-shaping
  view parts; if node-position response shaping turns out impossible in
  topcoat 0.4, that is a design-stopping deviation to report back, not to
  work around silently.
- **Client router.** Ships inside the vendored loader. Behavior:
  - Intercept clicks on same-origin `<a>` without modifier keys, `target`,
    `download`, or `data-tcs-reload` (the opt-out), and not pure-hash links.
  - Fetch the target URL with `X-Topcoat-Svelte: data`; expect JSON. Anything
    else (redirect to another content type, network error, non-page route) →
    fall back to a full navigation (`location.assign`). Progressive
    enhancement: without JS, links are just links.
  - On success: dynamic-import the module, unmount the current page component,
    mount the new one into the SAME page hydration root with the new props
    (client render — hydration is only for the initial document), then
    `history.pushState`, scroll to top (or to `#hash` target), and let
    `<svelte:head>` apply on mount (title etc.).
  - `popstate` re-runs the same data fetch for the restored URL.
  - Only active when the document contains a page hydration root; island-only
    documents get no interception.
- **State semantics (v1, documented).** The whole page component tree is
  swapped; Svelte-side layout component state does NOT survive navigation
  (SvelteKit preserves layout instances; we don't yet). Islands outside the
  page root are untouched.
- **Prefetch (v1).** On `pointerover`/`focus` of an interceptable link,
  fire the data fetch and module import ahead of the click, cached per URL for
  the session's navigation lifetime. Failures during prefetch are silent (the
  click path retries or falls back).
- **Testing.** Rust: a page route returns the JSON contract under the header
  and the HTML document without it (both feature modes). Browser E2E: click
  navigation between two Svelte pages without a full reload (assert via an
  in-page marker surviving navigation), correct URL/title, back/forward works,
  fallback on a non-page link.

## Dev live reload (the `dev` cargo feature) — HMR v1

Editing a `.svelte` file while the app runs updates the browser with **no
rustc and no Node in the loop**: the compiler is a Rust library inside the
server process. v1 is whole-page live reload (component-state-preserving hot
swap is a later refinement). Pinned contract:

- **Feature.** `dev` on `topcoat-svelte`, off by default, never meant for
  production (documented loudly). It adds `rsvelte_core` and a file watcher
  (`notify`) as runtime dependencies of the app.
- **Shared compile crate.** The macro's compile pipeline (graph walk, oxc
  specifier rewriting, hashing, name derivation) moves into a new library
  crate `topcoat-svelte-compile`, used by the proc-macro at build time and by
  the dev reloader at run time. One implementation, two callers; the macro's
  observable output stays byte-identical (existing tests prove it).
- **Source tracking.** Every registered module records its absolute source
  path (the macro already knows it). The dev watcher watches exactly those
  paths.
- **Overlay registry.** A `RwLock` overlay maps a module's ORIGINAL served
  URL to its latest recompiled JS (client and server text). The serve route
  consults the overlay first; URLs never change in dev (the overlay swaps
  content under the stable URL), so no client-side remapping exists. With
  `dev` enabled, module/runtime responses are served `Cache-Control:
  no-cache` instead of immutable.
- **Recompile scope.** On a changed file: recompile it and every ancestor
  that (transitively) imports it — child URL embedding means ancestors'
  text changes too. Specifier rewriting maps each child to its ORIGINAL
  stable URL (from the registry), preserving the graph. Debounce bursts.
- **SSR interplay.** The overlay updates server JS as well and bumps a
  global generation counter; each thread-local engine lazily rebuilds its
  context when it observes a stale generation, so the next server render
  uses the fresh component.
- **Client.** With `dev` on, `script()` additionally emits a small dev
  script (vendored like the loader) that opens an `EventSource` on
  `GET /_topcoat-svelte/dev/events` (SSE, served by the same `serve` route)
  and calls `location.reload()` on a `change` event.
- **Failure mode.** A failed recompile keeps the last good overlay entry,
  logs the compiler diagnostic to the terminal (`eprintln!` convention), and
  emits an `error` SSE event whose payload the dev script `console.error`s.
  No reload is triggered until a compile succeeds.
- **Activation.** One explicit call in the app (e.g.
  `topcoat_svelte::dev::watch()` invoked from main when the feature is on)
  starts the watcher; it is a no-op question for prod builds because the
  feature gates the module entirely.

## Open questions deliberately deferred to Phase 2

- SSR + hydration (JS engine embedding: Boa vs rquickjs) — see topcoat repo discussion.
- `$app`-style stores; typed props.
- Serving extracted CSS instead of `css: Injected`.
- Topcoat CLI / dev-server niceties (watch `.svelte` files — `include_str!` already
  gives rebuild-on-change through cargo).
