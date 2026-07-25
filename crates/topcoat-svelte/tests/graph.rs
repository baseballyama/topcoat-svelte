//! Exercises `.svelte`-imports-`.svelte` module graphs through the `svelte!`
//! macro: a parent -> child -> grandchild chain (with a look-alike string
//! literal that must not be rewritten) and a shared-stem case where two
//! different files named `Button.svelte` are used from one entry.

use std::collections::HashMap;

use topcoat::context::CxTestBuilder;
use topcoat::view::view;
use topcoat_svelte::{CompiledModule, SvelteComponent, compiled_modules, script, svelte};

static PARENT: SvelteComponent = svelte!("./fixtures/graph/Parent.svelte");
static BUTTONS: SvelteComponent = svelte!("./fixtures/graph/Buttons.svelte");

/// Groups every registered module by component name.
fn modules_by_name() -> HashMap<&'static str, Vec<&'static CompiledModule>> {
    let mut map: HashMap<&'static str, Vec<&'static CompiledModule>> = HashMap::new();
    for module in compiled_modules() {
        map.entry(module.name()).or_default().push(module);
    }
    map
}

fn only<'a>(
    modules: &'a HashMap<&'static str, Vec<&'static CompiledModule>>,
    name: &str,
) -> &'a CompiledModule {
    let entries = modules
        .get(name)
        .unwrap_or_else(|| panic!("no module named {name}"));
    assert_eq!(entries.len(), 1, "expected exactly one {name} module");
    entries[0]
}

#[test]
fn parent_child_grandchild_are_all_registered() {
    // Reference the handles so their whole graphs are linked and registered.
    let _ = (PARENT.name(), BUTTONS.name());
    let modules = modules_by_name();
    assert!(modules.contains_key("Parent"));
    assert!(modules.contains_key("Child"));
    assert!(modules.contains_key("Grandchild"));
}

#[test]
fn child_specifiers_are_rewritten_and_lookalike_literals_are_not() {
    let _ = PARENT.name();
    let modules = modules_by_name();
    let parent = only(&modules, "Parent");
    let child = only(&modules, "Child");
    let grandchild = only(&modules, "Grandchild");

    let child_url = format!("/_topcoat-svelte/c/{}", child.filename());
    let grandchild_url = format!("/_topcoat-svelte/c/{}", grandchild.filename());

    // The parent imports the child by its served, content-hashed URL, never the
    // original relative specifier.
    assert!(
        parent.js().contains(&child_url),
        "parent JS is missing the child URL {child_url}"
    );
    assert!(!parent.js().contains("'./Child.svelte'"));
    assert!(!parent.js().contains("\"./Child.svelte\""));

    // The string literal that merely looks like a specifier is left intact.
    assert!(parent.js().contains("./fake.svelte"));

    // The chain continues: the child imports the grandchild by URL.
    assert!(child.js().contains(&grandchild_url));
    assert!(!child.js().contains("'./Grandchild.svelte'"));
}

#[test]
fn transitive_cache_busting_links_parent_hash_to_child_hash() {
    let _ = PARENT.name();
    let modules = modules_by_name();
    let parent = only(&modules, "Parent");
    let child = only(&modules, "Child");
    // The parent's compiled output embeds the child's current hash, so any
    // change to the child changes the URL, hence the parent's own hash.
    assert!(parent.js().contains(&format!("Child-{}.js", child.hash())));
}

#[test]
fn same_stem_different_files_do_not_collide() {
    let _ = BUTTONS.name();
    let modules = modules_by_name();
    let buttons = only(&modules, "Buttons");

    let variants = modules.get("Button").expect("Button modules registered");
    assert_eq!(variants.len(), 2, "expected two distinct Button modules");
    assert_ne!(
        variants[0].hash(),
        variants[1].hash(),
        "the two Button files must hash differently"
    );

    // Both distinct URLs appear in the entry, and each resolves to a registered
    // module (distinct filenames keep them from colliding in the registry).
    for variant in variants {
        let url = format!("/_topcoat-svelte/c/{}", variant.filename());
        assert!(
            buttons.js().contains(&url),
            "Buttons JS is missing variant URL {url}"
        );
    }
}

#[tokio::test]
async fn parent_island_renders_and_imports_are_covered() {
    let _ = PARENT.name();
    let cx = CxTestBuilder::new().build();
    let cx = &cx;
    let html = view! { cx =>
        (script())
        (PARENT.island(cx, &serde_json::json!({ "count": 2 })))
    }
    .unwrap()
    .render(cx);

    assert!(html.contains("type=\"importmap\""));
    assert!(html.contains("data-tcs-module=\"/_topcoat-svelte/c/Parent-"));
    assert!(html.contains("<script type=\"application/json\">{\"count\":2}</script>"));
}
