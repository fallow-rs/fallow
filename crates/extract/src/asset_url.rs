//! Shared asset-reference URL normalization.
//!
//! Used by parsers that emit side-effect imports from user-authored asset
//! references: Angular `@Component({ templateUrl, styleUrl })`, HTML
//! `<script src>` / `<link href>`, Vue `<script src>`, and SFC `<style src>`.
//!
//! Browsers, Vite, Parcel, Angular's compiler, Vue external scripts, and
//! SFC style loaders all resolve these references relative to the document or
//! component file whether or not they start with `./`. Fallow's downstream
//! specifier classifier, however, treats any string not starting with `.`, `/`, or
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
        assert_eq!(
            normalize_asset_url("data:text/javascript;base64,YWJj"),
            "data:text/javascript;base64,YWJj"
        );
    }

    #[test]
    fn scoped_package_unchanged() {
        assert_eq!(
            normalize_asset_url("@shared/header.html"),
            "@shared/header.html"
        );
    }

    #[test]
    fn empty_string_edge_case() {
        assert_eq!(normalize_asset_url(""), "./");
    }
}
