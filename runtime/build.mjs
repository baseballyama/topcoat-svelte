// Maintainer-only build. Vendors the Svelte client runtime and the island
// loader into crates/topcoat-svelte/runtime/dist as committed ES modules, so
// application `cargo build`s never need Node.
//
// Usage: cd runtime && pnpm install && node build.mjs
//
// Layout produced (mirrors the served URL namespace):
//   dist/loader.js                     -> /_topcoat-svelte/loader.js
//   dist/runtime/svelte.js             -> /_topcoat-svelte/runtime/svelte.js
//   dist/runtime/client.js             -> /_topcoat-svelte/runtime/client.js
//   dist/runtime/disclose-version.js   -> /_topcoat-svelte/runtime/disclose-version.js
//   dist/runtime/flags-legacy.js       -> /_topcoat-svelte/runtime/flags-legacy.js
//   dist/runtime/chunk-*.js            -> shared code split out of the entries
//
// Splitting is required: the entries share one copy of the Svelte client
// runtime through a common chunk, so the runtime's module-level state is a
// singleton across every entry (mount, the component namespace, and the
// disclose-version side effect).

import { build } from "esbuild";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { rm, mkdir } from "node:fs/promises";

const here = dirname(fileURLToPath(import.meta.url));
const distDir = join(here, "..", "crates", "topcoat-svelte", "runtime", "dist");

await rm(distDir, { recursive: true, force: true });
await mkdir(distDir, { recursive: true });

// The Svelte runtime entries, bundled and code-split into dist/runtime.
await build({
  entryPoints: {
    svelte: join(here, "entry", "svelte.js"),
    client: join(here, "entry", "client.js"),
    "disclose-version": join(here, "entry", "disclose-version.js"),
    "flags-legacy": join(here, "entry", "flags-legacy.js"),
  },
  outdir: join(distDir, "runtime"),
  bundle: true,
  splitting: true,
  format: "esm",
  platform: "browser",
  conditions: ["browser"],
  minify: true,
  entryNames: "[name]",
  chunkNames: "chunk-[hash]",
  legalComments: "none",
  logLevel: "info",
});

// The loader, served at the namespace root. `svelte` stays external so the
// browser resolves it through the import map to dist/runtime/svelte.js.
await build({
  entryPoints: [join(here, "loader.js")],
  outfile: join(distDir, "loader.js"),
  bundle: true,
  format: "esm",
  platform: "browser",
  external: ["svelte", "svelte/*"],
  minify: true,
  legalComments: "none",
  logLevel: "info",
});

console.log("vendored Svelte runtime -> " + distDir);
