# Implementation notes (Phase 1)

This records where the implementation deviates from `DESIGN.md`, and why. Every
deviation was driven by what the published Topcoat 0.4 / rsvelte APIs actually
offer, verified against their real source.

## Deviations from the pinned contract

### 1. `script()` is a function returning a view value, not a `#[component]`

DESIGN describes `script()` as "a Topcoat `#[component]`". It is instead a plain
`pub fn script() -> SvelteScript`, where `SvelteScript` implements
`topcoat::view::NodeViewParts`.

Rationale: DESIGN's own usage is `(topcoat_svelte::script())` -- expression/node
position, not component-call position. A `#[component]` is an `async fn`
returning `Result<View>` (a future), which does not drop into node position
without `await`. A value implementing `NodeViewParts` is synchronous, needs no
`cx`, and renders exactly where the contract shows it. The emitted HTML (import
map then loader module) is unchanged.

### 2. `serve` is built from `RouteFn`, not the `#[route]` macro

DESIGN suggested "a `#[route]`-style handler". `serve` is a
`pub const serve: RouteFn` constructed from the public `RouteFn::new` +
`Path::new` + `Method::GET`, with a plain `fn` handler.

Rationale: topcoat-router 0.4 does **not** re-export the `#[route]` attribute
macro (it lives in the `topcoat` facade's `router` module, not in
`topcoat-router`), and building the `RouteFn` value directly is the idiomatic way
for a library to hand the application a route. Usage matches the contract exactly:
`router.route(topcoat_svelte::serve)`. The handler reads the requested file from
`uri(cx).path()` by stripping the `/_topcoat-svelte/` prefix, rather than using a
`#[path_param]` (which would also require the facade macro).

### 3. The import map (and runtime) covers legacy mode too

DESIGN left `svelte/internal/flags/legacy` optional ("if bundling it is awkward,
restrict Phase 1 to runes mode"). It is included. Compiling real components
through `rsvelte_core` shows the exact specifier sets:

- runes component -> `svelte/internal/disclose-version`, `svelte/internal/client`
- legacy component -> the two above **plus** `svelte/internal/flags/legacy`
- the loader -> `svelte` (for `mount`)

Bundling the legacy flag entry was not awkward, so Phase 1 supports both runes
and legacy single-file components. The import map has four keys: `svelte`,
`svelte/internal/client`, `svelte/internal/disclose-version`,
`svelte/internal/flags/legacy`. The `tests/importmap.rs` test compiles one runes
and one legacy fixture and asserts the map covers every bare specifier both
emit.

### 4. `island(cx, props)` accepts `cx` but does not use it yet

The contract signature `COUNTER.island(cx, &props)` is kept verbatim. In Phase 1
there is no server rendering, so `cx` is unused; it is retained for API stability
and future SSR. Props are serialized eagerly inside `island`.

### 5. Dependency on the `topcoat` facade

`topcoat-svelte` depends on the `topcoat` facade (public API) rather than the
individual `topcoat-view` / `topcoat-router` crates. This guarantees a single,
consistent `Cx`/`View` type and gives tests `view!` and `CxTestBuilder` through
one dependency. `topcoat-view` and `topcoat-router` remain declared in
`[workspace.dependencies]` (per DESIGN) but are reached through the facade.
`NodeViewParts` is public in topcoat-view 0.4.0, so the documented `Unescaped`
fallback was not needed.

## Implementation choices left open by DESIGN

- **Hashing.** SHA-256 truncated to the first 16 hex characters (8 bytes), for
  both the per-component URL hash (computed in the macro) and the runtime
  cache-bust hash (computed in `build.rs` over the vendored bytes).
- **Embedding the runtime.** A `build.rs`-generated table of
  `(served path, include_str!(...))` pairs, plus `RUNTIME_HASH`. No `include_dir`
  crate and no filesystem access at run time. `build.rs` emits
  `rerun-if-changed` for every vendored file.
- **Registration.** The `svelte!` expansion calls `inventory::submit!` inside the
  `static` initializer block (verified to link and register correctly), through
  the re-exported `topcoat_svelte::__private::inventory` path so the user crate
  needs no direct `inventory` dependency.
- **Macro path resolution.** The calling source file is found with
  `proc_macro::Span::call_site().local_file()` (stable on the 1.96 toolchain);
  `./`/`../` resolve against its directory, bare paths against
  `CARGO_MANIFEST_DIR`, absolute paths as-is. The path is `canonicalize`d, which
  doubles as the existence check.

## Vendored runtime

`runtime/build.mjs` (esbuild, maintainer-only) produces, committed under
`crates/topcoat-svelte/runtime/dist/`:

```
loader.js                          # island loader ( `svelte` kept external )
runtime/svelte.js                  # re-exports mount/unmount
runtime/client.js                  # re-exports svelte/internal/client
runtime/disclose-version.js        # self-contained side effect
runtime/flags-legacy.js            # legacy-mode flag side effect
runtime/chunk-*.js                 # shared code split out of the entries
```

Code splitting is load-bearing: the entries share one copy of the Svelte client
runtime through a common chunk, so the runtime's module-level state is a
singleton across `mount`, the component namespace, and the flags. `pnpm` pins
`svelte@5.56.4` (matching the version rsvelte targets) and `esbuild@0.25.0`.

## Things for the reviewer to scrutinize

- **Props escaping** (`src/escape.rs`): escapes `<`, `>`, `&`, U+2028, U+2029 as
  `\uXXXX`. These appear only inside JSON string values, so the escape is JSON-
  equivalent while preventing a `</script>` breakout. Covered by unit tests and
  an end-to-end island test with a malicious payload.
- **Import map coverage** (`tests/importmap.rs`): the check re-derives specifiers
  from the actually-registered compiled modules, so it will fail if a future
  rsvelte version emits a new bare specifier the map does not carry.
- **`inventory::submit!` in a `static` initializer**: relies on the linker
  keeping the referenced `static`'s initializer. In normal use the `SvelteComponent`
  is referenced by `island(...)`, so it is kept. This was validated with a
  standalone experiment before adopting it.

## Module graph (`.svelte` importing `.svelte`)

A component may import other components by relative path. The `svelte!` macro
compiles the entire reachable graph, not just the entry file. This lifts the
Phase 1 "single-file components only" non-goal.

### How specifier rewriting works, and why it is safe

rsvelte preserves user imports verbatim in its client output (e.g.
`import Child from './Child.svelte';`). The macro must turn that specifier into a
served URL without touching a look-alike string literal such as
`let note = "loaded from ./fake.svelte";`.

Rewriting is therefore **AST-based, not textual** (`rewrite.rs`):

1. The compiled JS is parsed with `oxc_parser` (pinned to the same `0.139` line
   `rsvelte_core` already builds against, so it adds no new version to the tree).
2. Only the top-level `Statement::ImportDeclaration`,
   `Statement::ExportAllDeclaration`, and `Statement::ExportNamedDeclaration`
   (when it has a `source`) are inspected. The specifier considered is the
   declaration's `source` string literal -- never an arbitrary string elsewhere
   in the program. A `.svelte` substring in a variable value or a text node is
   in a different AST node and is structurally unreachable from this walk, so it
   is impossible to rewrite by accident. This is covered by
   `rewrite::tests::ignores_svelte_substring_in_string_literals`,
   `rewrite::tests::rewrites_only_the_import_specifier_not_a_lookalike_literal`,
   and the integration test
   `graph::child_specifiers_are_rewritten_and_lookalike_literals_are_not`.
3. Only relative (`./`, `../`) specifiers ending in `.svelte` are collected;
   bare specifiers (`svelte/internal/client`) and non-`.svelte` relative imports
   are left for the import map / browser.
4. Each replacement uses the string literal's byte span (`source.span`, which
   spans the quotes), and edits are applied back-to-front so earlier offsets stay
   valid. Served URLs contain only URL-safe characters, so re-quoting them with
   single quotes needs no escaping.

If oxc cannot parse the compiled JS at all (`ParserReturn::panicked`), the macro
emits a clear compile error rather than silently missing an import. In practice
rsvelte output parses with zero diagnostics.

### Recursion, ordering, dedup, and cycles (`graph.rs`)

- The graph is built depth-first. A **child is compiled before its parent**, so
  the parent's content hash is computed *after* its child specifiers are
  rewritten to `/_topcoat-svelte/c/{Child}-{hash}.js`. This makes cache busting
  transitive: a change anywhere in the graph changes every ancestor's hash.
- **Dedup is by canonicalized path** (which implies identical content), stronger
  than the "dedupe by content" the brief suggested: a diamond import compiles the
  shared module once. Across separate `svelte!` calls the same child may be
  submitted twice with the same `{name}-{hash}.js` filename; the registry's
  `HashMap` keying already collapses that, so it is harmless.
- **Cycles are a compile error.** A cycle has no valid hashing order (each
  module's hash would depend on the other's URL, which depends on its hash). The
  DFS stack detects re-entry and reports the chain
  (`cyclic \`.svelte\` imports are not supported: A -> B -> A`). Tested by
  `graph::tests::detects_import_cycles`.
- **Same-stem, different-file** components do not collide: differing content
  yields different hashes, hence different served filenames. Tested by
  `graph::same_stem_different_files_do_not_collide`.

### URL construction coupling

The child URL baked into a parent's JS is built in the macro from the literal
prefix `/_topcoat-svelte/c/` (`graph::MODULE_URL_PREFIX`). The macro cannot
depend on the `topcoat-svelte` crate (that would be circular), so it cannot read
the crate's `NAMESPACE`. The runtime-side test
`component::tests::serve_url_matches_macro` pins `SvelteComponent::module_url` to
exactly this shape, so the two definitions cannot drift apart unnoticed.

### Macro crate structure

The `svelte!` macro grew from one `lib.rs` into `lib.rs` (the proc-macro entry
and token generation), `resolve.rs` (path resolution, naming, hashing),
`rewrite.rs` (the oxc-based specifier extraction/rewriting), and `graph.rs` (the
recursive graph builder), following the repo's file-per-module style.

## Stage 2: SSR + hydration (the `ssr` feature)

Implements the `DESIGN.md` "Phase 2: SSR + hydration" contract on `rquickjs`,
per the `docs/phase2-ssr-spike.md` verdict. Off by default; enabling it
server-renders islands and hydrates them.

### Macro: always compile both outputs

`svelte!` now calls `rsvelte_core::compile_both` (one shared parse/analyze) and
embeds both the client and server JavaScript in each `CompiledModule`. The
server text gets the *same* specifier rewriting as the client text, but computed
independently: a child's served URL is resolved once (from the client imports)
and then applied to the server text using that text's own oxc-located import
spans (client and server emit the same `import` specifiers at different byte
offsets). The module hash -- and therefore the served URL -- is still the hash
of the *client* text only, so URLs are unchanged from a client-only build and
the no-feature output stays byte-identical
(`component::tests::island_html_is_byte_identical_without_ssr`). Server output
always compiles even when `ssr` is off; that is dead weight in the binary, which
the contract accepts in exchange for a single macro path.

### Engine: `rquickjs` module loader (reviewer focus)

`ssr.rs` (feature-gated) runs the server code in QuickJS. Two things are worth
scrutiny:

- **Module resolution.** A custom `Resolver`/`Loader` pair maps in-engine module
  names to source:
  - bare `svelte/server` and `svelte/internal/server` -> the vendored server
    runtime bundle (`runtime/server/*.js`, embedded via `RUNTIME_FILES`);
  - a component's key -- which is its served URL, byte-for-byte the specifier the
    macro rewrote into both client and server JS -> that component's server JS
    from the registry (`registry::server_source`);
  - a relative specifier (only the server bundle's split chunks import these) ->
    its file name, looked up under `runtime/server/`.
  The resolver reduces relatives to their basename and passes bare/absolute names
  through unchanged. This is the same idea as the client's import map, enforced
  in Rust instead of the browser. Because the rewritten URL is the engine key,
  the module graph resolves identically on both sides -- validated end to end
  (`graph_island_ssrs_with_children_resolved`).
- **Engine lifecycle.** One engine per thread via `thread_local!` holding an
  `OnceCell<Result<RquickjsEngine, String>>`: setup (build runtime, set loader,
  evaluate the render harness) runs once on first render on a thread and a setup
  failure is remembered rather than retried. The harness (`HARNESS`) imports
  `render` from `svelte/server`, dynamically `import()`s a component by key
  (caching it in a JS `Map`), `JSON.parse`s the exact embedded props string, and
  returns `render(...).body`. Renders drive the QuickJS job queue synchronously
  with `Promise::finish`. The `Runtime` is kept in the engine struct only to
  outlive the `Context`. QuickJS caches modules by resolved name, so a child
  pulled in as a parent's dependency and later rendered as its own island is not
  re-declared -- confirmed in the spike scratch.

### Props identity

The string embedded in the island's `<script type="application/json">` is the
single source of truth: `island` passes that exact string to the engine (parsed
by `JSON.parse` there) and the same string seeds hydration on the client, so the
hydration precondition (identical props) holds by construction. The existing
`escape::to_script_json` escaping (e.g. `<` -> `<`) is JSON-valid, so the
engine parses it back to the same value -- verified in the spike scratch
(`{"name":"a<b"}` renders `a&lt;b`).

### Failure mode

`island` calls `server_render`, which under `ssr` maps any engine error to the
crate's `eprintln!` convention plus an empty `("", String::new())`, so the island
degrades to a client-rendered placeholder. An SSR bug therefore never 500s a
page (`ssr_error_falls_back_to_client_rendering`).

### Vendored server runtime (deviation to report)

`runtime/build.mjs` gained a server bundle (`svelte/server` +
`svelte/internal/server`, esbuild-split, `platform: "neutral"`) under
`runtime/dist/runtime/server/`. Regenerating `dist` also changed two *client*
files that the brief hoped would stay stable: `runtime/svelte.js` (now also
re-exports `hydrate`, which the loader needs) and, as a consequence,
`runtime/client.js` and the shared client chunk (esbuild now pulls `hydrate` and
its dependencies into the client runtime, and the content-hashed chunk name
changed). `disclose-version.js`, `flags-legacy.js`, and the small disclose chunk
stayed byte-identical. This is unavoidable: hydration must ship in the client
bundle. `loader.js` also changed (the `hydrate` branch).

### Example and feature wiring

`counter-example` gains a non-default `ssr` feature forwarding to
`topcoat-svelte/ssr`, so the plain `cargo build --workspace` needs no C compiler;
`cargo run -p counter-example --features ssr` demonstrates SSR. The example's
page test asserts the SSR markers only under `#[cfg(feature = "ssr")]`.

## Stage 3: Svelte pages (the `page()` API)

Implements the `DESIGN.md` "Stage 3: Svelte pages" contract: `SvelteComponent::page`
renders a full `<!doctype html>` document from one component tree, with the
`#[page]` Rust fn as SvelteKit's `load()`. Works with and without `ssr`.

### Head-hook API shape (choice to report)

The contract left the Rust extra-head hook to the implementer (builder or a
second method). Chosen: a **consuming builder method on the returned value**,
`Page::with_head(self, head: topcoat::view::View) -> Self`. `page()` returns a
`Page` that is directly usable as a `NodeViewParts` node; `.with_head(...)`
chains optional Rust head content:

```rust,ignore
view! { cx => (PAGE.page(cx, &props).with_head(view! { cx =>
    <meta name="description" content=(description)>
}?)) }
```

Rationale: `page()` already returns a node value, so a chained builder reads
naturally in `view!` node position and keeps the no-extra-head case a bare
`PAGE.page(cx, &props)`. The head is a `View` (not `impl NodeViewParts`) because
`Page` must store it in a field until render; `View` is the concrete, public,
object-safe-to-store type that already implements `NodeViewParts`, so the extra
head still flows through the normal (escaping) view path. It renders lazily in
`Page::into_view_parts`, where the `cx` is available, and is spliced into
`<head>` after `script()`'s output and the component's `<svelte:head>`. The
caller unwraps the `view!` macro's `Result` with `?` (natural inside a
`-> Result` `#[page]` fn).

### `SsrEngine::render` now returns `{ head, body }`

The engine trait's output changed from `String` (body) to a small
`SsrOutput { head, body }` struct, per the contract's head-extraction requirement.
The render harness returns Svelte's `render()` result object directly; the Rust
side reads its `head` and `body` string fields via `rquickjs::Object::get`
(rquickjs implements no `FromJs` for tuples, so a `[head, body]` array decoded to
`(String, String)` does not compile -- reading the object's named fields is both
what works and what stays closest to Svelte's shape). `render_island` keeps
returning body-only (islands live in an existing document); `render_page` returns
both. `component_key_prefix()`'s per-call `String` allocation became the
compile-time `const COMPONENT_KEY_PREFIX`, pinned to `NAMESPACE` by a unit test
(Stage 2 review nit b).

### Hydration root shared; loader unchanged (contract held, zero deviation)

`island()` and `page()` both build the body's hydration root through one
`hydration_root(module_url, ssr_attr, server_html, props_json)` helper -- the
same `data-tcs-island` + `data-tcs-ssr` + embedded-props div, `display:contents`
so it spans the body without a layout box. Because a page's root is byte-for-byte
island-shaped, **the client loader needed zero changes**: its existing
`data-tcs-ssr ? hydrate : mount` branch hydrates a page as-is. This was the
contract's explicit success criterion, and it held.

### Server runtime excluded from the public route (Stage 2 review nit a)

`serve` now 404s any `runtime/server/*` path (`is_server_runtime`): the vendored
server runtime is embedded (build.rs bundles all of `runtime/dist`) only for the
`ssr` engine to load in-process, and must never be a browser asset. Client
runtime files and compiled modules stay served. Covered by a `serve.rs` unit test
on the predicate and a route-level `tests/serve.rs` that drives the actual
`serve` handler: `GET /_topcoat-svelte/runtime/server/server.js` -> 404 while
`loader.js`/`runtime/svelte.js` -> 200, and (under `ssr`) SSR still renders while
that path is 404. Driving the route needs a hand-built `http::request::Parts`, so
`http` was added as a **dev-dependency** of `topcoat-svelte` (test-only).

### Deviations from the Stage 3 contract

None material. The document is emitted as `<!doctype html><html><head>…` with no
`lang`/`<meta charset>` baked in -- those belong to the app (via `with_head` or
the component's `<svelte:head>`), keeping `page()` unopinionated. `page()` takes
`cx` for symmetry with `island()` but does not use it at construction (props
serialize eagerly, SSR runs on the current thread's engine); it is retained for
API stability, matching the island precedent.
