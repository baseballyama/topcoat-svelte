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
