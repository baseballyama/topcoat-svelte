//! The [`script`] component that wires the Svelte runtime into a page.

use topcoat::context::Cx;
use topcoat::view::{NodeViewParts, PartsWriter};

use crate::runtime::RUNTIME_HASH;

/// The `(bare specifier, served runtime file)` pairs that make up the import
/// map. The specifiers are exactly those the rsvelte-compiled client output and
/// the island loader import.
const IMPORT_MAP: &[(&str, &str)] = &[
    ("svelte", "runtime/svelte.js"),
    ("svelte/internal/client", "runtime/client.js"),
    (
        "svelte/internal/disclose-version",
        "runtime/disclose-version.js",
    ),
    ("svelte/internal/flags/legacy", "runtime/flags-legacy.js"),
];

/// Emits the `<script>` tags that let a page host Svelte islands: an import map
/// pointing the Svelte specifiers at the vendored runtime, followed by the
/// island loader module.
///
/// Place it in `<head>`. The import map must come before any module script, and
/// putting it in the head also lets the loader start resolving as early as
/// possible.
///
/// ```ignore
/// use topcoat::view::view;
///
/// view! {
///     <head>(topcoat_svelte::script())</head>
/// }
/// ```
#[must_use]
pub fn script() -> SvelteScript {
    SvelteScript
}

/// The value returned by [`script`]. Usable in node position inside
/// [`view!`](topcoat::view::view).
pub struct SvelteScript;

impl NodeViewParts for SvelteScript {
    fn into_view_parts(self, _cx: &Cx, parts: &mut PartsWriter<'_>) {
        // Built from fixed specifiers, the namespace, and the content hash, so
        // there is nothing to escape.
        parts.push_str_unescaped(markup());
    }
}

/// The import map + loader `<script>` markup. Shared with
/// [`SvelteComponent::page`](crate::SvelteComponent::page), which places it in
/// the document head it builds.
pub(crate) fn markup() -> String {
    let imports: serde_json::Map<String, serde_json::Value> = IMPORT_MAP
        .iter()
        .map(|(specifier, file)| {
            let url = format!("{}/{file}?v={RUNTIME_HASH}", crate::NAMESPACE);
            ((*specifier).to_owned(), serde_json::Value::String(url))
        })
        .collect();
    let import_map = serde_json::json!({ "imports": imports });
    // Serializing a map of static strings never fails.
    let import_map = serde_json::to_string(&import_map).unwrap();

    format!(
        "<script type=\"importmap\">{import_map}</script>\
         <script type=\"module\" src=\"{}/loader.js?v={RUNTIME_HASH}\"></script>",
        crate::NAMESPACE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_import_map_before_loader() {
        let html = markup();
        let map_at = html.find("type=\"importmap\"").unwrap();
        let loader_at = html.find("loader.js").unwrap();
        assert!(map_at < loader_at);
        for (specifier, _) in IMPORT_MAP {
            assert!(html.contains(&format!("\"{specifier}\"")));
        }
        assert!(html.contains(&format!("loader.js?v={RUNTIME_HASH}")));
    }

    #[test]
    fn import_map_is_valid_json_with_all_specifiers() {
        let html = markup();
        let start = html.find("type=\"importmap\">").unwrap() + "type=\"importmap\">".len();
        let end = html[start..].find("</script>").unwrap() + start;
        let parsed: serde_json::Value = serde_json::from_str(&html[start..end]).unwrap();
        let imports = parsed.get("imports").unwrap().as_object().unwrap();
        for (specifier, _) in IMPORT_MAP {
            assert!(imports.contains_key(*specifier), "missing {specifier}");
        }
    }
}
