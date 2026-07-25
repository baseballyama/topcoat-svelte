//! Server-side rendering of islands through an embedded JavaScript engine.
//!
//! Enabled by the `ssr` feature. rsvelte compiles each component to server
//! JavaScript alongside the client output; this module runs that server code in
//! an embedded [QuickJS](https://bellard.org/quickjs/) engine (via `rquickjs`)
//! to produce the island's HTML on the server, which the client then hydrates.
//!
//! The engine loads modules through a custom resolver/loader: the bare
//! `svelte/server` and `svelte/internal/server` specifiers resolve to the
//! vendored server runtime, and a component's module key -- its served URL,
//! identical to the specifier the client uses -- resolves to that component's
//! server JavaScript in the registry. The module graph therefore resolves the
//! same way in the engine as it does in the browser.
//!
//! One engine is created per thread ([`thread_local!`]) and reused across
//! renders: setup (building the runtime and evaluating the render harness) is
//! comparatively expensive, while a warm render is on the order of microseconds.

use std::cell::OnceCell;

use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::module::Declared;
use rquickjs::{CatchResultExt, Context, Ctx, Function, Module, Object, Promise, Runtime};

use crate::{registry, runtime};

/// A Svelte server render: the `<svelte:head>` content and the body HTML.
///
/// `head` carries whatever the component placed in `<svelte:head>` (empty when
/// it declares none); `body` is the component markup, including Svelte's
/// hydration boundary markers. Islands use only `body`; a full page also lifts
/// `head` into the document head.
pub(crate) struct SsrOutput {
    pub(crate) head: String,
    pub(crate) body: String,
}

/// Server-renders a component through the embedded engine.
trait SsrEngine {
    /// Renders the component served at `module_key` with `props_json` (the exact
    /// JSON string embedded in the island's props script) into its
    /// [`SsrOutput`].
    fn render(&self, module_key: &str, props_json: &str) -> Result<SsrOutput, String>;
}

/// Renders an island's server body HTML on the current thread's engine.
///
/// `module_key` is the component's served URL and `props_json` is the exact JSON
/// string that will also seed hydration on the client. Returns the body HTML, or
/// an error describing why the render (or engine setup) failed. The component's
/// `<svelte:head>` output is discarded here; islands live inside an existing
/// document, so only [`render_page`] lifts head content out.
pub(crate) fn render_island(module_key: &str, props_json: &str) -> Result<String, String> {
    render(module_key, props_json).map(|output| output.body)
}

/// Renders a page's server HTML on the current thread's engine, keeping both the
/// `<svelte:head>` content and the body so a full document can place each where
/// it belongs.
pub(crate) fn render_page(module_key: &str, props_json: &str) -> Result<SsrOutput, String> {
    render(module_key, props_json)
}

/// Drives the current thread's engine to render `module_key` with `props_json`.
fn render(module_key: &str, props_json: &str) -> Result<SsrOutput, String> {
    ENGINE.with(|cell| match cell.get_or_init(RquickjsEngine::new) {
        Ok(engine) => engine.render(module_key, props_json),
        Err(err) => Err(format!("SSR engine failed to initialize: {err}")),
    })
}

thread_local! {
    /// One reused engine per thread. `OnceCell` so setup happens on first use
    /// and a setup failure is remembered rather than retried every render.
    static ENGINE: OnceCell<Result<RquickjsEngine, String>> = const { OnceCell::new() };
}

/// The render harness, evaluated once per engine. It imports `render` from the
/// vendored `svelte/server` and exposes `renderKey`, which dynamically imports a
/// component by its module key (cached after first use), parses the props from
/// the exact embedded JSON string, and returns Svelte's render result. Its
/// `head` carries `<svelte:head>` content; its `body` carries the component
/// markup with Svelte's hydration boundary markers.
const HARNESS: &str = r#"
import { render } from 'svelte/server';
const cache = new Map();
export async function renderKey(key, propsJson) {
    let component = cache.get(key);
    if (component === undefined) {
        const module = await import(key);
        component = module.default;
        cache.set(key, component);
    }
    return render(component, { props: JSON.parse(propsJson) });
}
"#;

/// The global name the harness's `renderKey` function is stashed under so each
/// render can look it up.
const RENDER_FN: &str = "__tcs_renderKey";

/// A QuickJS engine with the Svelte server runtime and render harness loaded.
struct RquickjsEngine {
    // The runtime must outlive the context; both are dropped when the thread
    // ends. `Context` keeps its own reference to the runtime, so this field is
    // just here to tie their lifetimes together.
    _runtime: Runtime,
    context: Context,
}

impl RquickjsEngine {
    /// Builds the engine: a fresh runtime with the module resolver/loader, a
    /// context, and the render harness evaluated with its `renderKey` stashed on
    /// the global object.
    fn new() -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|err| err.to_string())?;
        runtime.set_loader(EngineResolver, EngineLoader);
        let context = Context::full(&runtime).map_err(|err| err.to_string())?;

        context.with(|ctx| -> Result<(), String> {
            let declared = Module::declare(ctx.clone(), "__tcs_ssr_harness", HARNESS)
                .catch(&ctx)
                .map_err(|err| err.to_string())?;
            let (module, promise) = declared.eval().catch(&ctx).map_err(|err| err.to_string())?;
            promise
                .finish::<()>()
                .catch(&ctx)
                .map_err(|err| err.to_string())?;
            let render_fn: Function = module
                .namespace()
                .and_then(|ns| ns.get("renderKey"))
                .map_err(|err| err.to_string())?;
            ctx.globals()
                .set(RENDER_FN, render_fn)
                .map_err(|err| err.to_string())?;
            Ok(())
        })?;

        Ok(Self {
            _runtime: runtime,
            context,
        })
    }
}

impl SsrEngine for RquickjsEngine {
    fn render(&self, module_key: &str, props_json: &str) -> Result<SsrOutput, String> {
        self.context.with(|ctx| -> Result<SsrOutput, String> {
            let render_fn: Function = ctx
                .globals()
                .get(RENDER_FN)
                .map_err(|err| err.to_string())?;
            let promise: Promise = render_fn
                .call((module_key, props_json))
                .catch(&ctx)
                .map_err(|err| err.to_string())?;
            // The harness resolves to Svelte's render result object; read its
            // `head` and `body` string fields.
            let result: Object = promise
                .finish::<Object>()
                .catch(&ctx)
                .map_err(|err| err.to_string())?;
            let head: String = result.get("head").map_err(|err| err.to_string())?;
            let body: String = result.get("body").map_err(|err| err.to_string())?;
            Ok(SsrOutput { head, body })
        })
    }
}

/// Normalizes module specifiers for the engine. Bare `svelte/*` specifiers and
/// absolute component keys pass through unchanged; a relative specifier (the
/// vendored server runtime's split chunks) is reduced to its file name, which
/// the loader looks up directly.
struct EngineResolver;

impl Resolver for EngineResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        _base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        let resolved = if name.starts_with('.') {
            name.rsplit('/').next().unwrap_or(name).to_string()
        } else {
            name.to_string()
        };
        Ok(resolved)
    }
}

/// Supplies module source for a resolved name.
struct EngineLoader;

impl Loader for EngineLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<Module<'js, Declared>> {
        let source = engine_source(name)
            .ok_or_else(|| rquickjs::Error::new_loading_message(name, "unknown SSR module"))?;
        Module::declare(ctx.clone(), name, source)
    }
}

/// Resolves a module name to its source: the vendored server runtime for the
/// bare `svelte/server` / `svelte/internal/server` specifiers and their shared
/// chunks, or a component's server JavaScript for its module key.
fn engine_source(name: &str) -> Option<&'static str> {
    match name {
        "svelte/server" => runtime::runtime_file("runtime/server/server.js"),
        "svelte/internal/server" => runtime::runtime_file("runtime/server/internal-server.js"),
        key if key.starts_with(COMPONENT_KEY_PREFIX) => registry::server_source(key),
        chunk => runtime::runtime_file(&format!("runtime/server/{chunk}")),
    }
}

/// The prefix every component module key starts with (`/_topcoat-svelte/c/`).
/// Kept as a compile-time constant so matching a key allocates nothing per
/// render; a unit test pins it against [`crate::NAMESPACE`].
const COMPONENT_KEY_PREFIX: &str = concat!("/_topcoat-svelte", "/c/");

#[cfg(test)]
mod tests {
    use super::COMPONENT_KEY_PREFIX;

    /// `COMPONENT_KEY_PREFIX` is spelled out as a literal (a `const` cannot call
    /// `format!`), so this pins it to the runtime-built prefix to catch any drift
    /// from [`crate::NAMESPACE`].
    #[test]
    fn component_key_prefix_matches_namespace() {
        assert_eq!(COMPONENT_KEY_PREFIX, format!("{}/c/", crate::NAMESPACE));
    }
}
