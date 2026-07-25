//! The [`serve`] route that serves the vendored runtime and compiled modules.

use std::borrow::Cow;

use topcoat::context::Cx;
use topcoat::router::{
    Body, HeaderValue, IntoResponse, Method, Path, RouteFn, RouteFuture, StatusCode, header, uri,
};

use crate::{registry, runtime};

const JS_CONTENT_TYPE: HeaderValue = HeaderValue::from_static("text/javascript; charset=utf-8");
const CACHE_CONTROL: HeaderValue = HeaderValue::from_static("public, max-age=31536000, immutable");

/// The route that serves every `topcoat-svelte` asset under
/// `/_topcoat-svelte/{*file}`: the vendored Svelte runtime, the island loader,
/// and each compiled component module.
///
/// Register it on the router alongside your own routes:
///
/// ```ignore
/// use topcoat::router::Router;
///
/// let router = Router::builder()
///     .route(topcoat_svelte::serve)
///     .discover()
///     .build();
/// ```
///
/// All responses are served with `Content-Type: text/javascript` and an
/// immutable one-year cache, since every URL is content-hashed.
#[allow(non_upper_case_globals)]
pub const serve: RouteFn = RouteFn::new(
    Method::GET,
    Cow::Borrowed(Path::new("/_topcoat-svelte/{*file}")),
    handle,
);

fn handle<'cx>(cx: &'cx Cx, _body: Body) -> RouteFuture<'cx> {
    Box::pin(async move {
        let path = uri(cx).path();
        let file = path
            .strip_prefix(crate::NAMESPACE)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or_default();

        let js = match file.strip_prefix("c/") {
            Some(module) => registry::lookup(module),
            // The server runtime (`runtime/server/*`) is embedded only for the
            // `ssr` engine to load in-process; it is never a browser asset, so it
            // stays off the public route even though `build.rs` bundles it.
            None if is_server_runtime(file) => None,
            None => runtime::runtime_file(file),
        };

        match js {
            Some(js) => (
                [
                    (header::CONTENT_TYPE, JS_CONTENT_TYPE),
                    (header::CACHE_CONTROL, CACHE_CONTROL),
                ],
                js,
            )
                .into_response(cx),
            None => (StatusCode::NOT_FOUND, "not found").into_response(cx),
        }
    })
}

/// Whether a requested file is part of the SSR-only server runtime, which must
/// not be served over the public route.
fn is_server_runtime(file: &str) -> bool {
    file.starts_with("runtime/server/")
}

#[cfg(test)]
mod tests {
    use super::is_server_runtime;

    #[test]
    fn server_runtime_files_are_not_public() {
        assert!(is_server_runtime("runtime/server/server.js"));
        assert!(is_server_runtime("runtime/server/internal-server.js"));
        // Client runtime and compiled modules stay publicly served.
        assert!(!is_server_runtime("runtime/svelte.js"));
        assert!(!is_server_runtime("runtime/client.js"));
        assert!(!is_server_runtime("loader.js"));
    }
}
