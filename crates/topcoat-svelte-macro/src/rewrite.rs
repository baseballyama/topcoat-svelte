//! Locating and rewriting `.svelte` import specifiers in compiled client JS.
//!
//! rsvelte preserves a component's `import Child from './Child.svelte'` in its
//! emitted output. To turn that into a served module URL, the specifier string
//! must be replaced -- but a naive text substitution would also corrupt a
//! look-alike string literal such as `let s = "./fake.svelte"`. To stay precise,
//! the JS is parsed with oxc and only the string literal that is the *source* of
//! a static `import` / `export ... from` declaration is considered.

use oxc_allocator::Allocator;
use oxc_ast::ast::Statement;
use oxc_parser::Parser;
use oxc_span::SourceType;

/// A relative `.svelte` import specifier found in a piece of compiled JS,
/// together with the byte range of the string literal (quotes included) that
/// spelled it.
pub(crate) struct SvelteImport {
    /// The specifier text, e.g. `./Child.svelte` or `../ui/Panel.svelte`.
    pub(crate) specifier: String,
    /// Byte offset of the opening quote in the source.
    pub(crate) start: usize,
    /// Byte offset just past the closing quote in the source.
    pub(crate) end: usize,
}

/// Finds every relative `.svelte` specifier that is the source of a top-level
/// `import` or `export ... from` declaration in `js`.
///
/// Only the string literal that names a module is inspected, so a `.svelte`
/// substring inside ordinary code (a variable's value, a text node) is never
/// reported. Bare specifiers (`svelte/internal/client`) and non-`.svelte`
/// relative imports are left for the import map / browser to resolve and are
/// not returned. Returns an error only if oxc cannot parse the JS at all.
pub(crate) fn svelte_imports(js: &str) -> Result<Vec<SvelteImport>, String> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, js, SourceType::mjs()).parse();
    if parsed.panicked {
        return Err("could not parse the compiled JavaScript to resolve its \
                    `.svelte` imports"
            .to_owned());
    }

    let mut imports = Vec::new();
    for statement in &parsed.program.body {
        let source = match statement {
            Statement::ImportDeclaration(decl) => Some(&decl.source),
            Statement::ExportAllDeclaration(decl) => Some(&decl.source),
            Statement::ExportNamedDeclaration(decl) => decl.source.as_ref(),
            _ => None,
        };
        let Some(source) = source else { continue };
        let specifier = source.value.as_str();
        if is_relative_svelte(specifier) {
            imports.push(SvelteImport {
                specifier: specifier.to_owned(),
                start: source.span.start as usize,
                end: source.span.end as usize,
            });
        }
    }
    Ok(imports)
}

/// A relative specifier (`./` or `../`) that names a `.svelte` file. Bare
/// package specifiers are excluded: there is no filesystem location to compile
/// them from, so they remain unsupported.
fn is_relative_svelte(specifier: &str) -> bool {
    (specifier.starts_with("./") || specifier.starts_with("../")) && specifier.ends_with(".svelte")
}

/// Replaces each import specifier's string literal with a fresh single-quoted
/// `url`, returning the rewritten JS.
///
/// `replacements` pairs a located [`SvelteImport`] with the URL its module is
/// served from. The served URLs contain only URL-safe characters (no quotes or
/// backslashes), so single-quoting them needs no escaping. Edits are applied
/// from the end of the source backwards so earlier byte offsets stay valid.
pub(crate) fn rewrite_specifiers(js: &str, replacements: &[(SvelteImport, String)]) -> String {
    let mut ordered: Vec<&(SvelteImport, String)> = replacements.iter().collect();
    ordered.sort_by_key(|entry| std::cmp::Reverse(entry.0.start));

    let mut out = js.to_owned();
    for (import, url) in ordered {
        out.replace_range(import.start..import.end, &format!("'{url}'"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_static_import_and_export_sources() {
        let js = "import 'svelte/internal/disclose-version';\n\
                  import * as $ from 'svelte/internal/client';\n\
                  import Child from './Child.svelte';\n\
                  export { default as Panel } from '../ui/Panel.svelte';\n\
                  export * from './All.svelte';\n";
        let mut found: Vec<String> = svelte_imports(js)
            .unwrap()
            .into_iter()
            .map(|i| i.specifier)
            .collect();
        found.sort();
        assert_eq!(
            found,
            vec![
                "../ui/Panel.svelte".to_owned(),
                "./All.svelte".to_owned(),
                "./Child.svelte".to_owned(),
            ]
        );
    }

    #[test]
    fn ignores_svelte_substring_in_string_literals() {
        let js = "import Child from './Child.svelte';\n\
                  let msg = \"this is not ./fake.svelte really\";\n\
                  const p = './also-fake.svelte';\n";
        let found = svelte_imports(js).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].specifier, "./Child.svelte");
    }

    #[test]
    fn ignores_bare_and_non_svelte_specifiers() {
        let js = "import 'svelte/internal/client';\n\
                  import x from './helper.js';\n\
                  import pkg from 'some/Widget.svelte';\n";
        assert!(svelte_imports(js).unwrap().is_empty());
    }

    #[test]
    fn rewrites_only_the_import_specifier_not_a_lookalike_literal() {
        let js = "import Child from './Child.svelte';\n\
                  let msg = \"see ./Child.svelte for details\";\n";
        let imports = svelte_imports(js).unwrap();
        let replacements: Vec<(SvelteImport, String)> = imports
            .into_iter()
            .map(|i| (i, "/_topcoat-svelte/c/Child-0011223344556677.js".to_owned()))
            .collect();
        let out = rewrite_specifiers(js, &replacements);
        assert!(out.contains("import Child from '/_topcoat-svelte/c/Child-0011223344556677.js';"));
        // The look-alike string literal is untouched.
        assert!(out.contains("\"see ./Child.svelte for details\""));
    }

    #[test]
    fn rewrites_multiple_specifiers() {
        let js = "import A from './A.svelte';\nimport B from './nested/B.svelte';\n";
        let imports = svelte_imports(js).unwrap();
        let replacements: Vec<(SvelteImport, String)> = imports
            .into_iter()
            .map(|i| {
                let url = match i.specifier.as_str() {
                    "./A.svelte" => "/_topcoat-svelte/c/A-aaaaaaaaaaaaaaaa.js",
                    _ => "/_topcoat-svelte/c/B-bbbbbbbbbbbbbbbb.js",
                };
                (i, url.to_owned())
            })
            .collect();
        let out = rewrite_specifiers(js, &replacements);
        assert!(out.contains("import A from '/_topcoat-svelte/c/A-aaaaaaaaaaaaaaaa.js';"));
        assert!(out.contains("import B from '/_topcoat-svelte/c/B-bbbbbbbbbbbbbbbb.js';"));
    }
}
