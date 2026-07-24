// Island loader. Served at /_topcoat-svelte/loader.js and injected by
// `topcoat_svelte::script()`. On load it finds every island marker, reads its
// module URL and props, dynamically imports the compiled component, and mounts
// it. `mount` and every `svelte*` specifier resolve through the import map that
// `script()` emits before this module.
import { mount } from "svelte";

function mountIsland(el) {
  if (el.hasAttribute("data-tcs-mounted")) return;
  const moduleUrl = el.getAttribute("data-tcs-module");
  if (!moduleUrl) return;
  el.setAttribute("data-tcs-mounted", "");

  let props = {};
  const json = el.querySelector(':scope > script[type="application/json"]');
  if (json && json.textContent) {
    try {
      props = JSON.parse(json.textContent);
    } catch (err) {
      console.error("[topcoat-svelte] could not parse props for", moduleUrl, err);
      return;
    }
  }

  import(moduleUrl)
    .then((mod) => {
      mount(mod.default, { target: el, props });
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

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", mountAll, { once: true });
} else {
  mountAll();
}
