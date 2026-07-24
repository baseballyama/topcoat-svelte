//! The vendored Svelte client runtime, embedded at build time.
//!
//! `build.rs` reads the committed `runtime/dist` directory and generates the
//! [`RUNTIME_FILES`] table (served path -> file contents) and [`RUNTIME_HASH`]
//! (a content hash used to cache-bust the runtime URLs).

include!(concat!(env!("OUT_DIR"), "/runtime_files.rs"));

/// Looks up a vendored runtime file by its served path (relative to the
/// `/_topcoat-svelte/` namespace root, e.g. `runtime/client.js` or `loader.js`).
pub(crate) fn runtime_file(path: &str) -> Option<&'static str> {
    RUNTIME_FILES
        .iter()
        .find(|(candidate, _)| *candidate == path)
        .map(|(_, contents)| *contents)
}
