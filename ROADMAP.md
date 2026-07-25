# Roadmap: a SvelteKit-class experience on a Rust server

North star: an application developer writes **views in Svelte and server logic
in Rust**, and gets the experience they expect from SvelteKit / Next.js —
file/module-based routing, SSR by default with hydration, client-side
navigation, form actions, and a fast dev loop — from a single `cargo run`,
with no Node.js anywhere.

Positioning: this is **not** a SvelteKit clone. Topcoat stays the application
framework (routing, data loading, auth, sessions — see topcoat's
"functions, not middlewares" philosophy); Svelte is the view layer. The
SvelteKit concepts map as:

| SvelteKit | here |
|---|---|
| `+page.svelte` | a `.svelte` component owned by a `#[page]` |
| `+page.server.js` `load()` | the `#[page]` Rust fn body (`Cx`-based) |
| `+layout.svelte` | a `.svelte` layout component (`{@render children()}`) |
| form `actions` | Rust handlers on the same route (POST) |
| `$types` | generated TS types from the Rust props structs |
| Vite dev server | `topcoat dev` + in-process rsvelte recompilation |

## Stage 1 — CSR islands (shipped)

`svelte!` + islands + module graph. Svelte components embed in `view!` pages,
client-rendered. Committed through `cfb4e04`.

## Stage 2 — SSR + hydration for islands (next)

Islands render their HTML on the server (rquickjs executing rsvelte's server
output behind a cargo feature `ssr`) and hydrate on the client. Design proven
by the spike (`docs/phase2-ssr-spike.md`): byte-identical HTML vs Node,
~50k renders/sec warm, hydration verified in a real browser.

- The engine sits behind a small internal trait so a pure-Rust engine (Boa
  after the upstream fix, or a future rsvelte-native subset renderer) can slot
  in without touching the rest.
- Per-worker `thread_local!` engine with the bundle pre-evaluated; cold-start
  per request is off the table (5 ms vs 0.02 ms).

## Stage 3 — Svelte pages

The whole page body is one Svelte component tree, not an island in a `view!`
shell. A `#[page]` fn plays the role of SvelteKit's `load()`: it computes
props in Rust (DB, auth, `Cx`), and hands them to a page-level `.svelte`
component that is SSR'd and hydrated. Layouts become Svelte components
receiving `children` snippets, mirroring topcoat's nested layouts. Navigation
is MPA (full reload) at this stage. This is already the Next.js
pages-router feel: SSR by default, per-route server data.

## Stage 4 — client-side navigation (Inertia-style)

A small client router: intercept same-origin link clicks, fetch the next
page's props as JSON plus its component module URL (the same `#[page]` route
answering a `X-Topcoat-Svelte: data` request), swap the page component while
preserving shared layout component state, manage scroll/focus/history,
prefetch on hover. Progressive enhancement: without JS every link still works
as MPA. The Inertia.js protocol is the proven prior art here — server-driven
routing with client-side component swaps — rather than porting SvelteKit's
router.

## Stage 5 — the rest of the experience

- **Form actions**: POST to the same route handled by a Rust action fn;
  a `use:enhance`-style client helper for no-reload submissions with
  validation errors flowing back as typed props.
- **Dev loop**: because the Svelte compiler is a Rust *library inside the
  server*, `topcoat dev` can recompile edited `.svelte` files at runtime and
  hot-swap the served module + re-render — **no rustc and no Node in the
  loop** for view edits; only Rust changes pay for a cargo rebuild. (Prod
  keeps the current compile-time embedding.) This can land any time after
  Stage 2 and is the single biggest DX lever.
- **Typed bridge**: generate `.d.ts` for page/island props from the Rust
  structs (serde + schemars) and type-check components against them with
  rsvelte's svelte-check.
- Streaming SSR, `<svelte:head>` management, view transitions — later.

## Known boundaries (stated honestly)

- **No npm ecosystem.** There is no bundler or `node_modules` in an app
  build, so components can only import other `.svelte` files (and, later,
  vendored JS). A rolldown-based resolution story could lift this eventually;
  it is out of scope for now. TypeScript in `<script lang="ts">` is fine —
  rsvelte strips it.
- **SSR executes JS.** Full generality requires a JS engine (rquickjs today;
  C FFI). A pure-Rust path exists in two forms — Boa once its VM bug is fixed
  upstream (`spike/boa-issue-draft.md`), or a long-term rsvelte-native
  renderer for statically analyzable components with engine fallback — both
  slot behind the Stage 2 engine trait.
