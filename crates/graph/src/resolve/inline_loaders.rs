//! Webpack inline loader requests (`!raw-loader?opts!./file.js`).
//!
//! Webpack lets an import specifier name the loaders for one resource. The
//! request starts with an optional `!`, `!!` or `-!` prefix that disables
//! configured loaders. Loader segments follow, separated by `!`, and the last
//! segment is the resource. A loader segment and the resource can carry a
//! `?query`.
//!
//! A `!` is also a valid character in a file name. The resolver therefore
//! resolves a request without a prefix as a plain path first, and uses the
//! loader syntax only when that path does not resolve to a file. A request
//! with a prefix is always a loader request.
//!
//! Loaders run at build time, so they get no graph edge. The analysis layer
//! credits loader packages as referenced tooling through
//! [`InlineLoaderRequest::loaders`].
//!
//! A loader replaces the exports of its resource: `raw-loader` gives text,
//! `worker-loader` gives a constructor, `css-loader` gives a class map. The
//! imported and re-exported bindings do not name exports of the resource, so
//! the graph gives the edge whole-module usage and keeps no re-export edge.
//! When the loader next to the resource is in
//! [`ASSET_LOADERS`], the bundle never runs the resource as code, so the
//! graph does not follow the imports of the resource. When a loader in the
//! chain is in [`THREAD_LOADERS`], the resource runs in another thread, so
//! the edge gets the same load kind as `new Worker(new URL(...))`.

use std::path::Path;

/// Prefixes that turn off configured loaders, longest first.
const LOADER_OVERRIDE_PREFIXES: &[&str] = &["-!", "!!", "!"];

/// Loaders that turn their resource into text, bytes or a URL, from the
/// documentation of each package. The bundle never runs the resource as code,
/// so the imports of the resource are not runtime imports. Loaders that run or
/// transform the resource as code (`babel-loader`, `ts-loader`, `css-loader`,
/// `sass-loader`, `html-loader`, `worker-loader` and similar) are not in this
/// table. The short names without `-loader` are the webpack 1 spelling.
const ASSET_LOADERS: &[&str] = &[
    // An `ArrayBuffer` with the file bytes.
    "arraybuffer-loader",
    "arraybuffer",
    // A base64 data URL.
    "base64-inline-loader",
    // A binary string with the file bytes.
    "binary-loader",
    "binary",
    // A Node `Buffer` with the file bytes.
    "buffer-loader",
    "buffer",
    // The public URL of a copy of the file.
    "file-loader",
    "file",
    // The file text.
    "raw-loader",
    "raw",
    // The SVG markup as a string.
    "svg-inline-loader",
    // A data URL of the SVG.
    "svg-url-loader",
    // The file text (RequireJS `text!` plugin).
    "text-loader",
    "text",
    // A data URL, or the public URL of a copy of the file.
    "url-loader",
    "url",
];

/// Loaders that bundle their resource as a separate script that runs in
/// another thread or global scope (a worker, a shared worker, a service
/// worker or a worklet), from the documentation of each package. The importer
/// gets a constructor, a register function or a URL, never the resource
/// exports. The graph gives such an edge the same load kind as
/// `new Worker(new URL(...))`. The short names without `-loader` are the
/// spelling that each package documents or the webpack 1 spelling.
const THREAD_LOADERS: &[&str] = &[
    // `comlink-loader`: moves the module into a Web Worker behind a proxy.
    "comlink-loader",
    // `service-worker-loader`: a function that registers a service worker.
    "service-worker-loader",
    // `serviceworker-loader`: a function that registers a service worker.
    "serviceworker-loader",
    "serviceworker",
    // `shared-worker-loader`: a `SharedWorker` constructor.
    "shared-worker-loader",
    "shared-worker",
    // `sharedworker-loader`: a `SharedWorker` constructor.
    "sharedworker-loader",
    "sharedworker",
    // `worker-loader`: a `Worker` constructor.
    "worker-loader",
    "worker",
    // `worker-plugin/loader`: the URL of a separate bundle for a worker.
    "worker-plugin",
    // `workerize-loader`: moves the module into a Web Worker behind a proxy.
    "workerize-loader",
    // `worklet-loader`: the URL of a script for `addModule` on a worklet.
    "worklet-loader",
];

/// One parsed webpack inline loader request.
#[derive(Debug, PartialEq, Eq)]
pub struct InlineLoaderRequest<'a> {
    /// Loader requests without their `?options`, in source order.
    loaders: Vec<&'a str>,
    /// The resource request, with its `?query` kept for the resolver.
    resource: &'a str,
    /// Whether the request starts with `!`, `!!` or `-!`.
    has_prefix: bool,
}

impl<'a> InlineLoaderRequest<'a> {
    /// Parse `specifier` as a webpack inline loader request.
    ///
    /// Returns `None` when the specifier has no `!` separator, is a URL, has
    /// an empty resource segment, or has the SystemJS plugin shape
    /// (`./style.css!css`: path loaders and a bare resource, no prefix).
    #[must_use]
    pub fn parse(specifier: &'a str) -> Option<Self> {
        if !specifier.contains('!') || specifier.contains("://") || specifier.starts_with("data:") {
            return None;
        }
        let stripped = LOADER_OVERRIDE_PREFIXES
            .iter()
            .find_map(|prefix| specifier.strip_prefix(prefix));
        let has_prefix = stripped.is_some();
        let request = stripped.unwrap_or(specifier);
        let (loader_chain, resource) = request.rsplit_once('!').unwrap_or(("", request));
        if resource.is_empty() {
            return None;
        }
        let loaders: Vec<&str> = loader_chain
            .split('!')
            .map(|segment| segment.split_once('?').map_or(segment, |(name, _)| name))
            .filter(|name| !name.is_empty())
            .collect();
        if !has_prefix && !is_path(resource) && loaders.iter().all(|loader| is_path(loader)) {
            return None;
        }
        Some(Self {
            loaders,
            resource,
            has_prefix,
        })
    }

    /// Return the loader request that `specifier` resolved through, given the
    /// file it resolved to.
    ///
    /// Returns `None` when `specifier` is not a loader request, or when it
    /// resolved as a plain path. The resolver tries a request without a prefix
    /// as a plain path first. A plain path keeps its `!` in the path of the
    /// target, while a resource segment never contains a `!`. So the request
    /// resolved as a plain path when `target` contains the first path segment
    /// of `specifier` that holds a `!`. `target` gives the path of the target,
    /// or `None` for a target that is not a file. It runs only for a loader
    /// request without a prefix.
    #[must_use]
    pub fn resolved<'p>(
        specifier: &'a str,
        target: impl FnOnce() -> Option<&'p Path>,
    ) -> Option<Self> {
        let request = Self::parse(specifier)?;
        if request.has_prefix {
            return Some(request);
        }
        let resolved_as_plain_path = target().is_some_and(|path| {
            specifier
                .split('/')
                .find(|segment| segment.contains('!'))
                .is_some_and(|segment| path.to_string_lossy().contains(segment))
        });
        (!resolved_as_plain_path).then_some(request)
    }

    /// Loader requests without their `?options`, in source order. A loader
    /// can be a package name or a path to a local loader file.
    #[must_use]
    pub fn loaders(&self) -> &[&'a str] {
        &self.loaders
    }

    /// The resource request, with its `?query`.
    #[must_use]
    pub const fn resource(&self) -> &'a str {
        self.resource
    }

    /// Whether the request must use the loader syntax (it has a prefix).
    #[must_use]
    pub const fn has_prefix(&self) -> bool {
        self.has_prefix
    }

    /// Whether the loader next to the resource reads it as an asset (text,
    /// bytes or a URL), so the bundle never runs the resource as code.
    ///
    /// Webpack runs the loaders from right to left, so only the last loader
    /// reads the resource file. In `raw-loader!sass-loader!./a.scss` Sass
    /// compiles the file and follows its imports first.
    #[must_use]
    pub fn reads_resource_as_asset(&self) -> bool {
        self.loaders
            .last()
            .is_some_and(|loader| ASSET_LOADERS.contains(&loader_package_name(loader)))
    }

    /// Whether a loader in the chain runs the resource in another thread or
    /// global scope, such as a worker or a worklet.
    ///
    /// Any position counts. A worker loader is a pitching loader: it compiles
    /// the rest of the request as a separate bundle, and the loaders to its
    /// left only see its output.
    #[must_use]
    pub fn runs_resource_in_another_thread(&self) -> bool {
        self.loaders
            .iter()
            .any(|loader| THREAD_LOADERS.contains(&loader_package_name(loader)))
    }
}

/// Whether a request segment is a relative or absolute path.
fn is_path(segment: &str) -> bool {
    segment.starts_with('.') || segment.starts_with('/')
}

/// The package name of a loader request (`raw-loader/dist/cjs.js` gives
/// `raw-loader`). A path to a local loader file stays as it is.
fn loader_package_name(loader: &str) -> &str {
    if is_path(loader) {
        return loader;
    }
    let mut parts = loader.splitn(3, '/');
    let first = parts.next().unwrap_or(loader);
    if first.starts_with('@') {
        parts
            .next()
            .map_or(loader, |second| &loader[..first.len() + 1 + second.len()])
    } else {
        first
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(specifier: &str) -> Option<(Vec<&str>, &str)> {
        InlineLoaderRequest::parse(specifier).map(|request| (request.loaders, request.resource))
    }

    #[test]
    fn parses_prefixes_loader_options_and_resource_query() {
        assert_eq!(
            parse("!raw-loader?esModule=false!./shim.js"),
            Some((vec!["raw-loader"], "./shim.js"))
        );
        assert_eq!(
            parse("!!style-loader!css-loader?modules!./a.css"),
            Some((vec!["style-loader", "css-loader"], "./a.css"))
        );
        assert_eq!(
            parse("-!./loaders/local.js!./worker.js"),
            Some((vec!["./loaders/local.js"], "./worker.js"))
        );
        assert_eq!(
            parse("raw-loader!./template.js?inline"),
            Some((vec!["raw-loader"], "./template.js?inline"))
        );
        assert_eq!(
            parse("@scope/loader!pkg/file"),
            Some((vec!["@scope/loader"], "pkg/file"))
        );
    }

    #[test]
    fn prefix_without_loaders_keeps_the_resource() {
        assert_eq!(parse("!!./file.js"), Some((vec![], "./file.js")));
    }

    #[test]
    fn plain_and_url_specifiers_are_not_loader_requests() {
        assert_eq!(parse("./file.js"), None);
        assert_eq!(parse("react"), None);
        assert_eq!(parse("https://example.com/a!b"), None);
        assert_eq!(parse("data:text/javascript,1!2"), None);
        assert_eq!(parse("raw-loader!"), None);
    }

    #[test]
    fn systemjs_plugin_suffix_is_not_a_loader_request() {
        assert_eq!(parse("./style.css!css"), None);
        assert_eq!(parse("../a/b.txt!text"), None);
        assert_eq!(
            parse("!./style.css!css"),
            Some((vec!["./style.css"], "css")),
            "a prefix always selects the loader syntax"
        );
    }

    #[test]
    fn a_plain_path_with_a_bang_resolves_as_a_plain_path() {
        let plain = Path::new("/project/src/we!rd.js");
        assert_eq!(
            InlineLoaderRequest::resolved("./we!rd.js", || Some(plain)),
            None
        );
        assert_eq!(
            InlineLoaderRequest::resolved("./we!rd", || Some(plain)),
            None
        );
        let dir = Path::new("/project/src/a!b/c.js");
        assert_eq!(
            InlineLoaderRequest::resolved("./a!b/c.js", || Some(dir)),
            None
        );
    }

    #[test]
    fn a_loader_request_resolves_to_its_resource() {
        let resource = Path::new("/project/src/shim.js");
        let request = InlineLoaderRequest::resolved("raw-loader!./shim.js", || Some(resource))
            .expect("a loader request");
        assert_eq!(request.loaders(), ["raw-loader"]);
        assert!(InlineLoaderRequest::resolved("raw-loader!./missing.js", || None).is_some());
        assert!(
            InlineLoaderRequest::resolved("!!./we!rd.js", || Some(Path::new("/p/we!rd.js")))
                .is_some(),
            "a prefix always selects the loader syntax"
        );
    }

    #[test]
    fn only_the_loader_next_to_the_resource_decides_asset_reads() {
        let asset = |specifier| {
            InlineLoaderRequest::parse(specifier)
                .expect("a loader request")
                .reads_resource_as_asset()
        };
        assert!(asset("!raw-loader!./a.js"));
        assert!(asset("raw!./a.js"));
        assert!(asset("file-loader?name=[name].[ext]!./a.png"));
        assert!(asset("url-loader/dist/cjs.js!./a.png"));
        assert!(asset("style-loader!raw-loader!./a.css"));
        assert!(!asset("raw-loader!sass-loader!./a.scss"));
        assert!(!asset("worker-loader!./worker.js"));
        assert!(!asset("!!style-loader!css-loader!./a.css"));
        assert!(!asset("babel-loader!./a.js"));
        assert!(!asset("!!./a.js"));
        assert!(!asset("-!./loaders/raw!./a.js"));
    }

    #[test]
    fn any_thread_loader_in_the_chain_runs_the_resource_in_another_thread() {
        let thread = |specifier| {
            InlineLoaderRequest::parse(specifier)
                .expect("a loader request")
                .runs_resource_in_another_thread()
        };
        assert!(thread("worker-loader!./w.js"));
        assert!(thread("worker!./w.js"));
        assert!(thread(
            "!!worker-loader?inline=fallback!babel-loader!./w.js"
        ));
        assert!(thread("sharedworker-loader?name=s!./s.js"));
        assert!(thread("shared-worker!./s.js"));
        assert!(thread("worklet-loader!./audio.js"));
        assert!(thread("workerize-loader!./w.js"));
        assert!(thread("comlink-loader?singleton!./w.js"));
        assert!(thread("service-worker-loader!./sw.js"));
        assert!(thread("serviceworker!./sw.js"));
        assert!(thread("worker-plugin/loader?esModule!./w.js"));
        assert!(!thread("raw-loader!./w.js"));
        assert!(!thread("babel-loader!./w.js"));
        assert!(!thread("./loaders/worker!./w.js"));
        assert!(!thread("!!./w.js"));
    }

    #[test]
    fn loader_package_name_keeps_the_scope() {
        assert_eq!(loader_package_name("raw-loader"), "raw-loader");
        assert_eq!(loader_package_name("raw-loader/dist/cjs.js"), "raw-loader");
        assert_eq!(loader_package_name("@scope/loader/x.js"), "@scope/loader");
        assert_eq!(loader_package_name("./loaders/raw"), "./loaders/raw");
    }
}
