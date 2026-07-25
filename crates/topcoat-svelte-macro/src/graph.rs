//! Compiling an entry component together with its transitive `.svelte` imports.
//!
//! Starting from the entry file, each component is compiled with rsvelte to both
//! client and server JavaScript, its relative `.svelte` import specifiers are
//! resolved and compiled recursively, and those specifiers are rewritten in both
//! emitted texts to the served URL of the child module. A child is compiled
//! before the parent that imports it, so the parent's content hash -- computed
//! *after* rewriting the client text -- folds in the child's hash and changes
//! whenever any transitive dependency changes.

use std::collections::HashMap;
use std::path::PathBuf;

use rsvelte_core::compiler::{CompileOptions, CssMode, compile_both};

use crate::resolve::{component_name, resolve_import, short_hash};
use crate::rewrite::{SvelteImport, rewrite_specifiers, svelte_imports};

/// The URL prefix compiled component modules are served under. This must stay
/// in step with `topcoat_svelte`'s `NAMESPACE` and
/// `SvelteComponent::module_url`, which build the same `/_topcoat-svelte/c/`
/// path; `topcoat-svelte`'s `serve_url_matches_macro` test guards the pairing.
const MODULE_URL_PREFIX: &str = "/_topcoat-svelte/c/";

/// A single compiled component in a [`Graph`].
pub(crate) struct Module {
    /// The PascalCase component name (its file stem).
    pub(crate) name: String,
    /// The content hash of the (rewritten) compiled client JavaScript. The URL
    /// is derived from the client hash so it is unchanged from client-only
    /// builds; the server text rides along under the same key.
    pub(crate) hash: String,
    /// The compiled client JavaScript, with child `.svelte` specifiers rewritten
    /// to served URLs.
    pub(crate) js: String,
    /// The compiled server JavaScript (for the `ssr` feature), with the same
    /// child specifiers rewritten to the same served URLs.
    pub(crate) server_js: String,
    /// The absolute path of the source file, for `include_str!`.
    pub(crate) abs_path: String,
}

impl Module {
    /// The URL this module is served from, matching
    /// `SvelteComponent::module_url`.
    fn module_url(&self) -> String {
        format!("{MODULE_URL_PREFIX}{}-{}.js", self.name, self.hash)
    }
}

/// An entry component and every distinct component reachable from it through
/// relative `.svelte` imports.
pub(crate) struct Graph {
    /// Every distinct module, ordered so a child always precedes the parents
    /// that import it.
    pub(crate) modules: Vec<Module>,
    /// Index into [`modules`](Graph::modules) of the entry component.
    entry: usize,
}

impl Graph {
    /// Compiles `entry` (an absolute, canonical path) and its transitive
    /// `.svelte` imports.
    pub(crate) fn build(entry: PathBuf) -> Result<Self, String> {
        let mut builder = Builder {
            compiled: HashMap::new(),
            modules: Vec::new(),
            stack: Vec::new(),
        };
        let entry = builder.compile(entry)?;
        Ok(Self {
            modules: builder.modules,
            entry,
        })
    }

    /// The entry component.
    pub(crate) fn entry(&self) -> &Module {
        &self.modules[self.entry]
    }
}

/// Depth-first graph walker. `compiled` dedupes shared imports by canonical
/// path so a component reached by several routes is compiled once; `stack`
/// holds the current path so a re-entry can be reported as a cycle.
struct Builder {
    compiled: HashMap<PathBuf, usize>,
    modules: Vec<Module>,
    stack: Vec<PathBuf>,
}

impl Builder {
    /// Compiles the component at canonical `path`, recursing into its children
    /// first, and returns its index in [`modules`](Builder::modules).
    fn compile(&mut self, path: PathBuf) -> Result<usize, String> {
        if let Some(&index) = self.compiled.get(&path) {
            return Ok(index);
        }
        if self.stack.contains(&path) {
            return Err(cycle_message(&self.stack, &path));
        }
        self.stack.push(path.clone());

        let name = component_name(&path)?;
        let abs_path = path
            .to_str()
            .ok_or_else(|| format!("path is not valid UTF-8: {}", path.display()))?
            .to_owned();
        let source = std::fs::read_to_string(&path)
            .map_err(|err| format!("could not read {}: {err}", path.display()))?;

        // One shared parse/analyze produces both the client and server output.
        // The server text is only used by the `ssr` feature, but compiling it
        // here keeps the two outputs derived from the same analysis.
        let options = CompileOptions {
            css: CssMode::Injected,
            name: Some(name.clone()),
            filename: Some(abs_path.clone()),
            ..Default::default()
        };
        let (client, server) = compile_both(&source, options)
            .map_err(|err| format!("failed to compile {}: {err}", path.display()))?;
        let client_js = client.js.code;
        let server_js = server.js.code;

        // Resolve every child from the client imports, recording each
        // specifier's served URL, then apply those URLs to the client text.
        let mut url_by_specifier: HashMap<String, String> = HashMap::new();
        let mut client_replacements = Vec::new();
        for import in svelte_imports(&client_js)? {
            let url = self.resolve_child_url(&path, &import.specifier)?;
            url_by_specifier.insert(import.specifier.clone(), url.clone());
            client_replacements.push((import, url));
        }
        let client_rewritten = rewrite_specifiers(&client_js, &client_replacements);

        // The server text imports the same children; rewrite its own spans to
        // the same URLs so the module graph resolves identically in the engine.
        let server_replacements: Vec<(SvelteImport, String)> = svelte_imports(&server_js)?
            .into_iter()
            .filter_map(|import| {
                url_by_specifier
                    .get(&import.specifier)
                    .map(|url| (import, url.clone()))
            })
            .collect();
        let server_rewritten = rewrite_specifiers(&server_js, &server_replacements);

        let hash = short_hash(&client_rewritten);

        self.stack.pop();
        let index = self.modules.len();
        self.modules.push(Module {
            name,
            hash,
            js: client_rewritten,
            server_js: server_rewritten,
            abs_path,
        });
        self.compiled.insert(path, index);
        Ok(index)
    }

    /// Resolves and compiles the child named by `specifier` (relative to
    /// `importer`), returning the URL its compiled module is served from.
    fn resolve_child_url(
        &mut self,
        importer: &std::path::Path,
        specifier: &str,
    ) -> Result<String, String> {
        let child = resolve_import(importer, specifier)
            .map_err(|err| format!("{specifier} (imported by {}): {err}", importer.display()))?;
        let child_index = self.compile(child)?;
        Ok(self.modules[child_index].module_url())
    }
}

/// Builds a readable message for a cyclic `.svelte` import, showing the chain
/// from where `repeated` first appears on the `stack` back to itself.
fn cycle_message(stack: &[PathBuf], repeated: &std::path::Path) -> String {
    let start = stack.iter().position(|p| p == repeated).unwrap_or(0);
    let mut chain: Vec<String> = stack[start..]
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    chain.push(repeated.display().to_string());
    format!(
        "cyclic `.svelte` imports are not supported: {}",
        chain.join(" -> ")
    )
}

#[cfg(test)]
mod tests {
    use super::Graph;
    use std::path::{Path, PathBuf};

    fn fixture(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(rel)
            .canonicalize()
            .unwrap_or_else(|e| panic!("fixture {rel}: {e}"))
    }

    fn find<'a>(graph: &'a Graph, name: &str) -> &'a super::Module {
        graph
            .modules
            .iter()
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("module {name} not in graph"))
    }

    #[test]
    fn detects_import_cycles() {
        let err = match Graph::build(fixture("fixtures/cycle/A.svelte")) {
            Ok(_) => panic!("expected a cycle error, but the graph built"),
            Err(err) => err,
        };
        assert!(err.contains("cyclic"), "expected a cycle error, got: {err}");
    }

    #[test]
    fn compiles_the_whole_graph() {
        let graph = Graph::build(fixture("fixtures/linear/Parent.svelte")).unwrap();
        let names: Vec<&str> = graph.modules.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"Parent"));
        assert!(names.contains(&"Child"));
        assert!(names.contains(&"Grandchild"));
        assert_eq!(graph.entry().name, "Parent");
    }

    #[test]
    fn orders_children_before_parents() {
        let graph = Graph::build(fixture("fixtures/linear/Parent.svelte")).unwrap();
        let index = |name: &str| graph.modules.iter().position(|m| m.name == name).unwrap();
        assert!(index("Grandchild") < index("Child"));
        assert!(index("Child") < index("Parent"));
    }

    #[test]
    fn rewrites_child_specifiers_to_served_urls() {
        let graph = Graph::build(fixture("fixtures/linear/Parent.svelte")).unwrap();
        let parent = find(&graph, "Parent");
        let child = find(&graph, "Child");

        // The child import is rewritten to the child's served URL...
        assert!(
            parent.js.contains(&child.module_url()),
            "parent JS is missing the child URL:\n{}",
            parent.js
        );
        // ...the original relative specifier is gone...
        assert!(!parent.js.contains("'./Child.svelte'"));
        // ...but the look-alike string literal is preserved verbatim.
        assert!(parent.js.contains("./fake.svelte"));

        // The grandchild import inside the child is rewritten too.
        let grandchild = find(&graph, "Grandchild");
        assert!(child.js.contains(&grandchild.module_url()));
        assert!(!child.js.contains("'./Grandchild.svelte'"));
    }

    #[test]
    fn parent_hash_folds_in_child_hashes() {
        // Because a parent is hashed after its child URLs are substituted, its
        // hash must differ from the hash of its own pre-rewrite output. A stable
        // way to check the transitive property: the parent's hash embeds the
        // child hash through the rewritten URL, so recompiling is deterministic
        // and the parent references the exact child hash currently in the graph.
        let graph = Graph::build(fixture("fixtures/linear/Parent.svelte")).unwrap();
        let parent = find(&graph, "Parent");
        let child = find(&graph, "Child");
        assert!(parent.js.contains(&format!("Child-{}.js", child.hash)));

        // Rebuilding yields identical hashes (determinism).
        let again = Graph::build(fixture("fixtures/linear/Parent.svelte")).unwrap();
        assert_eq!(find(&again, "Parent").hash, parent.hash);
        assert_eq!(find(&again, "Child").hash, child.hash);
    }
}
