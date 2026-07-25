# Phase 2 spike: executing rsvelte's server output in an embedded JS engine

Findings from the 2026-07-25 SSR feasibility spike. Verdict: **rquickjs works
end-to-end; Boa cannot run the Svelte server runtime today** because of an
upstream VM bug (minimal reproduction below).

## Setup

- Components compiled with `rsvelte_core` (`GenerateMode::Server` /
  `GenerateMode::Client`).
- `svelte/internal/server` (svelte 5.56.4) + the compiled component bundled with
  esbuild into a single self-contained IIFE script.
- Ground truth: the same components rendered by real Node with `svelte/server`'s
  `render()`.

rsvelte's server output is small and simple — `import * as $ from
'svelte/internal/server'` plus one exported render function pushing template
strings through `$.escape(...)`. All the SSR machinery lives in
`svelte/internal/server`; the whole question is whether an engine can run that
module.

## rquickjs 0.12 (QuickJS via C FFI) — works

- HTML output **matches Node byte-for-byte** across all tested cases: props,
  prop defaults, ternaries, scoped-CSS class emission, and the `<!--[-->` /
  `<!--]-->` hydration boundary markers.
- **No shims needed** for the synchronous render path. `process` / `document` /
  `Buffer` are only touched behind feature-detection; a defensive no-op
  `console` shim is recommended but not required.
- Performance: ~50,000 renders/sec warm (~0.02 ms/render) with a reused
  `Context` + pre-evaluated bundle; on this micro-benchmark it beat Node/V8
  (~38,000/sec). Cold setup is ~0.3 ms (engine) + ~5 ms (bundle eval), so
  re-creating the engine per request collapses throughput to ~200/sec —
  **engine reuse is mandatory** (e.g. one `Runtime`+`Context` with the bundle
  pre-evaluated per worker thread via `thread_local!`). No leak trend over
  100k renders.
- `Runtime`/`Context` are `Send + Sync`; execution serializes through an
  internal lock, hence the per-worker-instance recommendation.
- Cost: a C FFI dependency (`rquickjs-sys` builds QuickJS from C source). This
  does not violate the crate's own `#![forbid(unsafe_code)]` (it is a
  dependency), but it does add a C compiler to the app's build — a policy
  trade-off against the "pure Rust" story.

## Boa 0.21 (pure Rust) — blocked upstream

Boa panics (Rust `index out of bounds` at `vm/opcode/define/mod.rs:82`,
`PutLexicalValue` with empty `code_block.bindings`) inside Svelte's `SSRState`
constructor. Minimal reproduction (`spike/boa_minimal_repro.js`):

```js
class S {
  constructor() {
    let u = 1;
    this.f = () => u++;
  }
}
new S().f(); // Boa 0.21.1: panic — Node & QuickJS: 1
```

The trigger is a class **constructor** declaring a block-scoped binding that a
closure created in the constructor captures. The same shape in ordinary
functions or non-constructor methods works. This is exactly Svelte's
`SSRState` constructor (`let uid = 1; this.uid = () => ...`), which is on the
mandatory path of every render, and it reproduces at every esbuild target from
es2020 to esnext (it comes from the Svelte source, not downleveling). Options:
report upstream and re-evaluate after a fix, or AST-transform the pattern away
in our bundle. Waiting for the upstream fix is the realistic path.

## Hydration (verified in a real browser)

The Phase 1 island shape carries SSR with three small changes:

1. `island()` renders the server HTML (with its `<!--[-->`/`<!--]-->` markers)
   inside the island div instead of leaving it empty. Props keep flowing
   through the existing `<script type="application/json">` element and **must
   be identical** to the props used for the server render.
2. The loader calls `hydrate(mod.default, { target, props })` instead of
   `mount(...)`.
3. The vendored `runtime/svelte.js` entry additionally re-exports `hydrate`
   (no new importmap entries; `svelte/internal/client` already contains the
   hydration code).

Verified in headless Chrome: hydrating server HTML reuses the existing DOM (no
duplicate elements), no hydration-mismatch warnings, and reactivity works after
hydration.

## Recommendation

Build Phase 2 on **rquickjs** behind a cargo feature (e.g. `ssr`), keeping
CSR-only islands as the default; track the Boa bug upstream and re-evaluate a
pure-Rust engine once fixed.
