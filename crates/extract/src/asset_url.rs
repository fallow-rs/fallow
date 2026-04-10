//! Shared asset-reference URL normalization.
//!
//! Used by parsers that emit side-effect imports from user-authored asset
//! references: Angular `@Component({ templateUrl, styleUrl })`, HTML
//! `<script src>` / `<link href>`, and Vue/Svelte `<script src>`.
//!
//! Browsers, Vite, Parcel, Angular's compiler, and Vue/Svelte's SFC tooling
//! all resolve these references relative to the document or component file
//! whether or not they start with `./`. Fallow's downstream specifier
//! classifier, however, treats any string not starting with `.`, `/`, or
//! containing `://` as a bare npm package specifier, so bare filenames like
//! `'app.component.html'` or `'app.js'` are misclassified as unlisted
//! dependencies. Prepending `./` at extraction time aligns the emitted
//! specifier with the real semantics of the reference.

/// Normalize an asset-reference URL so bare filenames are treated as relative
/// paths, not npm package specifiers.
///
/// Paths that already start with `.` (relative), `/` (absolute), contain a
/// URL scheme (`://`), use a `data:` URI prefix, or use a scoped package
/// prefix (`@scope/...`) are returned unchanged. Everything else gets `./`
/// prepended.
///
/// The `data:` guard keeps this helper safe to call unconditionally even from
/// call sites that don't pre-filter via `is_remote_url`.
pub fn normalize_asset_url(url: &str) -> String {
    if url.starts_with('.')
        || url.starts_with('/')
        || url.contains("://")
        || url.starts_with("data:")
    {
        return url.to_string();
    }
    if url.starts_with('@') && url.contains('/') {
        return url.to_string();
    }
    format!("./{url}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_filename_gets_dot_slash() {
        assert_eq!(
            normalize_asset_url("app.component.html"),
            "./app.component.html"
        );
        assert_eq!(normalize_asset_url("app.js"), "./app.js");
        assert_eq!(normalize_asset_url("styles.css"), "./styles.css");
    }

    #[test]
    fn bare_subdir_gets_dot_slash() {
        assert_eq!(
            normalize_asset_url("templates/app.html"),
            "./templates/app.html"
        );
        assert_eq!(normalize_asset_url("assets/logo.svg"), "./assets/logo.svg");
    }

    #[test]
    fn dot_slash_unchanged() {
        assert_eq!(
            normalize_asset_url("./app.component.html"),
            "./app.component.html"
        );
    }

    #[test]
    fn parent_relative_unchanged() {
        assert_eq!(
            normalize_asset_url("../shared/app.html"),
            "../shared/app.html"
        );
    }

    #[test]
    fn absolute_path_unchanged() {
        assert_eq!(normalize_asset_url("/src/app.html"), "/src/app.html");
    }

    #[test]
    fn url_scheme_unchanged() {
        assert_eq!(
            normalize_asset_url("https://cdn.example.com/app.html"),
            "https://cdn.example.com/app.html"
        );
        assert_eq!(
            normalize_asset_url("http://example.com/script.js"),
            "http://example.com/script.js"
        );
    }

    #[test]
    fn data_uri_unchanged() {
        // `data:` URIs don't contain `://` but must not be prepended with `./`.
        // Defensive check lets the helper be called unconditionally from SFC
        // parsers that don't pre-filter remote/data URLs.
        assert_eq!(
            normalize_asset_url("data:text/javascript;base64,YWJj"),
            "data:text/javascript;base64,YWJj"
        );
    }

    #[test]
    fn scoped_package_unchanged() {
        // Scoped package path aliases (webpack/esbuild) should stay bare so
        // the resolver can handle them via node_modules / alias resolution.
        assert_eq!(
            normalize_asset_url("@shared/header.html"),
            "@shared/header.html"
        );
    }

    #[test]
    fn empty_string_edge_case() {
        // Empty asset URL is syntactically possible but semantically invalid.
        // Document current behavior: the normalizer prepends `./`, producing
        // `./` which the resolver will fail to match to a file.
        assert_eq!(normalize_asset_url(""), "./");
    }
}
