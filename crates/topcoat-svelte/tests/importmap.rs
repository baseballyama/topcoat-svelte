//! Asserts that the import map emitted by `script()` covers every bare module
//! specifier the compiled components actually import. Compiles both a runes and
//! a legacy fixture so the check spans both specifier sets.

use topcoat::context::CxTestBuilder;
use topcoat::view::view;
use topcoat_svelte::{SvelteComponent, compiled_modules, script, svelte};

static RUNES: SvelteComponent = svelte!("./fixtures/counter.svelte");
static LEGACY: SvelteComponent = svelte!("./fixtures/legacy.svelte");

#[tokio::test]
async fn import_map_covers_every_bare_specifier() {
    // Reference the handles so their modules are linked and registered.
    let _ = (RUNES.name(), LEGACY.name());

    let cx = CxTestBuilder::new().build();
    let cx = &cx;
    let head = view! { cx => (script()) }.unwrap().render(cx);
    let keys = import_map_keys(&head);
    assert!(keys.iter().any(|k| k == "svelte"));

    let mut specifiers: Vec<String> = compiled_modules()
        .flat_map(|module| bare_import_specifiers(module.js()))
        .collect();
    specifiers.sort();
    specifiers.dedup();
    assert!(!specifiers.is_empty(), "no modules were registered");

    for specifier in &specifiers {
        assert!(
            keys.iter().any(|k| k == specifier),
            "import map is missing `{specifier}`; keys = {keys:?}"
        );
    }

    // The two fixtures should exercise both the runes and the legacy runtime.
    assert!(specifiers.iter().any(|s| s == "svelte/internal/client"));
    assert!(
        specifiers
            .iter()
            .any(|s| s == "svelte/internal/flags/legacy")
    );
}

/// Parses the `imports` keys out of the `<script type="importmap">` element.
fn import_map_keys(head: &str) -> Vec<String> {
    let open = "type=\"importmap\">";
    let start = head.find(open).expect("import map present") + open.len();
    let end = start + head[start..].find("</script>").expect("import map closed");
    let json: serde_json::Value =
        serde_json::from_str(&head[start..end]).expect("import map is valid JSON");
    json["imports"]
        .as_object()
        .expect("imports object")
        .keys()
        .cloned()
        .collect()
}

/// Collects the bare (non-relative, non-absolute) specifiers imported by a piece
/// of JavaScript, reading the string that follows each `import`/`from` keyword.
fn bare_import_specifiers(js: &str) -> Vec<String> {
    let mut out = Vec::new();
    for keyword in ["import", "from"] {
        let mut cursor = 0;
        while let Some(offset) = js[cursor..].find(keyword) {
            let after = cursor + offset + keyword.len();
            cursor = after;
            let rest = js[after..].trim_start();
            let mut chars = rest.chars();
            let Some(quote) = chars.next() else { continue };
            if quote != '\'' && quote != '"' {
                continue;
            }
            let literal = &rest[quote.len_utf8()..];
            if let Some(end) = literal.find(quote) {
                let specifier = &literal[..end];
                if !specifier.starts_with('.') && !specifier.starts_with('/') {
                    out.push(specifier.to_owned());
                }
            }
        }
    }
    out
}
