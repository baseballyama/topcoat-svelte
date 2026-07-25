// Entry served for the bare `svelte` specifier. The island loader imports
// `mount` to instantiate a client-only island and `hydrate` to take over
// server-rendered (SSR) island markup.
export { mount, unmount, hydrate } from "svelte";
