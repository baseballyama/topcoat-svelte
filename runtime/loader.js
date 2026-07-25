// Island loader + client router. Served at /_topcoat-svelte/loader.js and
// injected by `topcoat_svelte::script()`. On load it finds every island marker,
// reads its module URL and props, dynamically imports the compiled component,
// and either mounts it (client-only island) or hydrates it (SSR island, marked
// with `data-tcs-ssr`).
//
// When the document is a Svelte *page* (its hydration root carries
// `data-tcs-page`), the loader also runs an Inertia-style client router:
// same-origin link clicks are intercepted, the target route is fetched with
// `X-Topcoat-Svelte: data` (the same `#[page]` fn answering with JSON instead of
// HTML), and the page component is swapped in place — no full reload. Anything
// unexpected (network error, non-page route, redirect to another content type)
// falls back to a real navigation, so without JS every link still works.
//
// `mount`/`hydrate`/`unmount` and every `svelte*` specifier resolve through the
// import map that `script()` emits before this module.
import { mount, hydrate, unmount } from "svelte";

// The request/response header pairing the client router with a page route.
const DATA_HEADER = "X-Topcoat-Svelte";
const DATA_HEADER_VALUE = "data";

// The currently mounted page component, so it can be unmounted on navigation:
// `{ el, instance }`, or null in an island-only document.
let currentPage = null;

// A per-document marker so an end-to-end test can prove navigation happened
// without a full reload: a full reload resets `navigations` to 0, a soft
// navigation increments it.
window.__topcoatSvelte = { navigations: 0 };

// -- initial island / page mounting --

function readProps(el, moduleUrl) {
  const json = el.querySelector(':scope > script[type="application/json"]');
  if (!json || !json.textContent) return {};
  try {
    return JSON.parse(json.textContent);
  } catch (err) {
    console.error("[topcoat-svelte] could not parse props for", moduleUrl, err);
    return null;
  }
}

function mountIsland(el) {
  if (el.hasAttribute("data-tcs-mounted")) return;
  const moduleUrl = el.getAttribute("data-tcs-module");
  if (!moduleUrl) return;
  el.setAttribute("data-tcs-mounted", "");

  const props = readProps(el, moduleUrl);
  if (props === null) return;

  // An SSR island/page already contains server-rendered markup; hydrate it in
  // place so the existing DOM is reused. A client-only one mounts from scratch.
  const create = el.hasAttribute("data-tcs-ssr") ? hydrate : mount;
  const isPage = el.hasAttribute("data-tcs-page");
  import(moduleUrl)
    .then((mod) => {
      const instance = create(mod.default, { target: el, props });
      if (isPage) currentPage = { el, instance };
    })
    .catch((err) => {
      console.error("[topcoat-svelte] could not mount island", moduleUrl, err);
    });
}

function mountAll() {
  for (const el of document.querySelectorAll("[data-tcs-island]")) {
    mountIsland(el);
  }
}

// -- client router --

// Fetched page data keyed by URL, warmed by prefetch and consumed by navigate,
// living for the document's lifetime.
const dataCache = new Map();

// Whether an anchor should be soft-navigated: same-origin, left-click-only, not
// opting out (`data-tcs-reload`), not a download, no `target`, and not a
// pure-hash link on the current page.
function interceptable(a) {
  if (!a || a.tagName !== "A") return false;
  if (a.hasAttribute("download") || a.hasAttribute("data-tcs-reload")) return false;
  if (a.target && a.target !== "_self") return false;
  const href = a.getAttribute("href");
  if (!href) return false;
  let url;
  try {
    url = new URL(a.href, location.href);
  } catch {
    return false;
  }
  if (url.origin !== location.origin) return false;
  // A pure-hash link (same path + query, only the fragment differs) is left to
  // the browser for in-page scrolling.
  if (url.pathname === location.pathname && url.search === location.search && url.hash) {
    return false;
  }
  return true;
}

// The cache key / fetch target for a URL: path + query + fragment.
function target(url) {
  return url.pathname + url.search + url.hash;
}

async function fetchData(href) {
  const res = await fetch(href, {
    headers: { [DATA_HEADER]: DATA_HEADER_VALUE },
    credentials: "same-origin",
  });
  const contentType = res.headers.get("content-type") || "";
  if (!res.ok || !contentType.includes("application/json")) {
    throw new Error("not a page data response");
  }
  const data = await res.json();
  if (!data || typeof data.module !== "string") {
    throw new Error("malformed page data");
  }
  return data;
}

// Warm the data + module for a URL ahead of a click. Failures are silent: the
// click path retries or falls back.
function prefetch(href) {
  if (dataCache.has(href)) return dataCache.get(href);
  const pending = fetchData(href)
    .then(async (data) => {
      await import(data.module);
      return data;
    })
    .catch(() => null);
  dataCache.set(href, pending);
  return pending;
}

// Monotonic token identifying the most recently started navigation. Each
// `navigate()` call captures its own value at entry; if a newer call has
// started by the time this one is ready to apply its result, it bails out
// silently so the latest click always wins over a slower, earlier one.
let navigationToken = 0;

// Soft-navigate to `href`. `push` records history (false when replaying a
// popstate); `hash` scrolls to a fragment target instead of the top.
async function navigate(href, { push = true, hash = "" } = {}) {
  const token = ++navigationToken;

  let data = null;
  const cached = dataCache.get(href);
  if (cached) data = await cached;
  if (!data) {
    try {
      data = await fetchData(href);
    } catch {
      location.assign(href);
      return;
    }
  }

  let mod;
  try {
    mod = await import(data.module);
  } catch {
    location.assign(href);
    return;
  }

  // A newer navigation has since started; let it win instead of applying this
  // now-stale result.
  if (token !== navigationToken) return;

  const el = currentPage ? currentPage.el : document.querySelector("[data-tcs-page]");
  if (!el) {
    location.assign(href);
    return;
  }

  // Swap the page component: unmount the old tree, then client-mount the new one
  // into the same root with the fetched props. Hydration is only for the initial
  // document; navigations render on the client.
  if (currentPage) {
    try {
      unmount(currentPage.instance);
    } catch (err) {
      console.error("[topcoat-svelte] could not unmount page", err);
    }
  }
  el.replaceChildren();

  // Past this point the old page is already gone, so any throw (a bad mount,
  // a `<svelte:head>` side effect, etc.) must still end in a real navigation
  // rather than leaving the root blank.
  try {
    const instance = mount(mod.default, { target: el, props: data.props });
    currentPage = { el, instance };

    if (push) history.pushState({ tcs: true }, "", href);

    const fragment = hash ? document.getElementById(decodeURIComponent(hash.slice(1))) : null;
    if (fragment) fragment.scrollIntoView();
    else window.scrollTo(0, 0);

    window.__topcoatSvelte.navigations += 1;
  } catch (err) {
    console.error("[topcoat-svelte] could not mount page", err);
    location.assign(href);
  }
}

function onClick(event) {
  if (event.defaultPrevented || event.button !== 0) return;
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  const anchor = event.target.closest && event.target.closest("a");
  if (!interceptable(anchor)) return;
  event.preventDefault();
  const url = new URL(anchor.href, location.href);
  navigate(target(url), { hash: url.hash });
}

function onPointerEnter(event) {
  const anchor = event.target.closest && event.target.closest("a");
  if (!interceptable(anchor)) return;
  prefetch(target(new URL(anchor.href, location.href)));
}

function onPopState() {
  navigate(target(new URL(location.href)), { push: false, hash: location.hash });
}

function activateRouter() {
  // Only a page document gets a client router; island-only pages are untouched.
  if (!document.querySelector("[data-tcs-page]")) return;
  document.addEventListener("click", onClick);
  document.addEventListener("pointerover", onPointerEnter);
  document.addEventListener("focusin", onPointerEnter);
  window.addEventListener("popstate", onPopState);
}

function boot() {
  mountAll();
  activateRouter();
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", boot, { once: true });
} else {
  boot();
}
