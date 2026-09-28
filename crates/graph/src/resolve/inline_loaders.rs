//! Webpack inline loader requests (`!raw-loader?opts!./file.js`).
//!
//! Webpack lets an import specifier name the loaders for one resource. The
//! request starts with an optional `!`, `!!` or `-!` prefix that disables
//! configured loaders. Loader segments follow, separated by `!`, and the last
//! segment is the resource. A loader segment and the resource can carry a
//! `?query`.
//!
//! The resolver resolves the resource like an ordinary specifier. Loaders run
//! at build time, so they get no graph edge. The analysis layer credits loader
//! packages as referenced tooling through [`inline_loader_names`].

/// Prefixes that turn off configured loaders, longest first.
const LOADER_OVERRIDE_PREFIXES: &[&str] = &["-!", "!!", "!"];

/// One parsed inline loader request.
#[derive(Debug, PartialEq, Eq)]
struct InlineLoaderRequest<'a> {
    /// Loader requests without their `?options`, in source order.
    loaders: Vec<&'a str>,
    /// The resource request, with its `?query` kept for the resolver.
    resource: &'a str,
}

/// Parse `specifier` as a webpack inline loader request.
///
/// Returns `None` when the specifier has no `!` separator, is a URL, or has an
/// empty resource segment.
fn parse_inline_loader_request(specifier: &str) -> Option<InlineLoaderRequest<'_>> {
    if !specifier.contains('!') || specifier.contains("://") || specifier.starts_with("data:") {
        return None;
    }
    let request = LOADER_OVERRIDE_PREFIXES
        .iter()
        .find_map(|prefix| specifier.strip_prefix(prefix))
        .unwrap_or(specifier);
    let (loader_chain, resource) = request.rsplit_once('!').unwrap_or(("", request));
    if resource.is_empty() {
        return None;
    }
    let loaders = loader_chain
        .split('!')
        .map(|segment| segment.split_once('?').map_or(segment, |(name, _)| name))
        .filter(|name| !name.is_empty())
        .collect();
    Some(InlineLoaderRequest { loaders, resource })
}

/// Return the resource of a webpack inline loader request, or the input
/// unchanged when it is not a loader request.
pub(super) fn strip_inline_loaders(specifier: &str) -> &str {
    parse_inline_loader_request(specifier).map_or(specifier, |request| request.resource)
}

/// Return the loader requests of a webpack inline loader specifier, without
/// their `?options`.
///
/// Returns an empty list when `specifier` is not a loader request. A loader
/// can be a package name or a path to a local loader file.
#[must_use]
pub fn inline_loader_names(specifier: &str) -> Vec<&str> {
    parse_inline_loader_request(specifier).map_or_else(Vec::new, |request| request.loaders)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(specifier: &str) -> Option<(Vec<&str>, &str)> {
        parse_inline_loader_request(specifier).map(|request| (request.loaders, request.resource))
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
    fn strip_and_names_leave_plain_specifiers_alone() {
        assert_eq!(strip_inline_loaders("./a.js"), "./a.js");
        assert_eq!(strip_inline_loaders("!raw-loader!./a.js"), "./a.js");
        assert!(inline_loader_names("./a.js").is_empty());
        assert_eq!(
            inline_loader_names("style-loader!css-loader?x!./a.css"),
            vec!["style-loader", "css-loader"]
        );
    }
}
