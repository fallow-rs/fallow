//! Package credit for `url()` values in stylesheets.
//!
//! A `url()` that names a file of a package, for example the `src` of an
//! `@font-face` that loads `@fontsource/inter/files/inter.woff2`, makes the
//! bundler read a file of that package. Vite and webpack `css-loader` try the
//! value relative to the stylesheet first and then as a package path, so both
//! `url(@scope/name/file)` and `url(name/file)` can load a package. A `~`
//! prefix (`url(~name/file)`) is a package path only.
//!
//! The package name becomes a package path reference, the same credit as a
//! `require.resolve('name')`. The credit counts only for a declared
//! dependency, so a value that is a local folder or a bundler alias, such as
//! `url(fonts/a.woff2)` or `url(@images/a.svg)`, reports nothing. Values that
//! start with `.`, `/` or `#`, values with a scheme, the `@/` alias and SCSS or
//! Less interpolation and variables name no package.

use std::sync::LazyLock;

/// A `url(...)` value: double-quoted, single-quoted, or unquoted.
static CSS_URL_RE: LazyLock<regex::Regex> =
    LazyLock::new(|| crate::static_regex(r#"url\(\s*(?:"([^"]*)"|'([^']*)'|([^\s"'()]+))\s*\)"#));

/// The package names of the `url()` values in `stripped`, a stylesheet with
/// its comments masked, each name once.
pub fn package_url_references(stripped: &str) -> Vec<String> {
    let mut packages: Vec<String> = Vec::new();
    if !stripped.contains("url(") {
        return packages;
    }
    for cap in CSS_URL_RE.captures_iter(stripped) {
        let Some(value) = cap.get(1).or_else(|| cap.get(2)).or_else(|| cap.get(3)) else {
            continue;
        };
        if let Some(package) = package_of_url(value.as_str().trim())
            && !packages.iter().any(|known| known == package)
        {
            packages.push(package.to_string());
        }
    }
    packages
}

/// The package name of `name/<path>` or `@scope/name/<path>`, with an optional
/// `~` prefix and `?query` or `#fragment` suffix. The path must not be empty.
fn package_of_url(value: &str) -> Option<&str> {
    if value.contains("#{") || value.contains('$') || value.contains("@{") {
        return None;
    }
    let path_end = value.find(['?', '#']).unwrap_or(value.len());
    let specifier = &value[..path_end];
    let specifier = specifier.strip_prefix('~').unwrap_or(specifier);
    let name_end = if specifier.starts_with('@') {
        let scope_end = specifier.find('/')?;
        if !is_npm_name_segment(&specifier[1..scope_end]) {
            return None;
        }
        scope_end + 1 + specifier[scope_end + 1..].find('/')?
    } else {
        specifier.find('/')?
    };
    let name = &specifier[..name_end];
    let unscoped_name = name.rsplit('/').next().unwrap_or(name);
    let path = &specifier[name_end + 1..];
    (is_npm_name_segment(unscoped_name) && is_plain_path(path)).then_some(name)
}

fn is_npm_name_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with(['.', '_'])
        && segment.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.' | b'_' | b'~')
        })
}

/// A non-empty file path with no backslash and no whitespace.
fn is_plain_path(path: &str) -> bool {
    !path.is_empty() && !path.bytes().any(|b| b == b'\\' || b.is_ascii_whitespace())
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;

    #[test]
    fn credits_scoped_unscoped_and_tilde_package_urls() {
        let css = r#"@font-face {
            src: url("@acme/font-sans/files/sans.woff2") format("woff2"),
                 url('@acme/font-mono/files/mono.woff2') format("woff2"),
                 url(@acme/icons/files/icons.woff2),
                 url(font-serif/files/serif.woff2),
                 url("~@acme/font-sans/files/sans-bold.woff2"),
                 url(~font-display/files/display.woff2);
        }"#;
        assert_eq!(
            package_url_references(css),
            vec![
                "@acme/font-sans",
                "@acme/font-mono",
                "@acme/icons",
                "font-serif",
                "font-display",
            ]
        );
    }

    #[test]
    fn strips_query_and_fragment_suffixes() {
        let css = r#".a {
            background: url("@acme/icons/sprite.svg#arrow");
            src: url("font-legacy/files/a.eot?#iefix"), url(font-query/a.woff?v=2);
        }"#;
        assert_eq!(
            package_url_references(css),
            vec!["@acme/icons", "font-legacy", "font-query"]
        );
    }

    #[test]
    fn skips_values_that_name_no_package() {
        let css = r#".a {
            background: url(./a.png), url(img.png), url(../b.png), url(/static/bg.png),
                url(#filter), url(data:image/png;base64,AAAA), url(https://example.com/a.png),
                url(//cdn.example.com/b.png), url("@/assets/a.png"), url(@App/assets/a.png),
                url("@acme/font-sans"), url(~font-display), url(_private/a.png),
                url(Images/a.png);
        }"#;
        assert!(
            package_url_references(css).is_empty(),
            "got {:?}",
            package_url_references(css)
        );
    }

    #[test]
    fn skips_scss_and_less_interpolation_and_variables() {
        let scss = r##".a {
            background: url("#{$base}/files/a.svg");
            mask: url($mask);
            border-image: url("@acme/icons/#{$name}.svg");
            list-style: url("@{base}/a.png");
        }"##;
        assert!(
            package_url_references(scss).is_empty(),
            "got {:?}",
            package_url_references(scss)
        );
    }

    #[test]
    fn lists_each_package_once() {
        let css = r#"@import url("@acme/theme/base.css");
            .a { background: url("@acme/theme/files/bg.png"); }"#;
        assert_eq!(package_url_references(css), vec!["@acme/theme"]);
    }
}
