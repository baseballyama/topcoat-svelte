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

## Open questions deliberately deferred to Phase 2

- SSR + hydration (JS engine embedding: Boa vs rquickjs) — see topcoat repo discussion.
- `$app`-style stores; typed props.
- Serving extracted CSS instead of `css: Injected`.
- Topcoat CLI / dev-server niceties (watch `.svelte` files — `include_str!` already
  gives rebuild-on-change through cargo).
