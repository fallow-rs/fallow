//! Specifier classification: bare specifiers, path aliases, and package name extraction.

/// Check if a bare specifier looks like a path alias rather than an npm package.
///
/// Path aliases (e.g., `@/components`, `~/lib`, `#internal`, `~~/utils`) are resolved
/// via tsconfig.json `paths` or package.json `imports`. They should not be cached
/// (resolution depends on the importing file's tsconfig context) and should return
/// `Unresolvable` (not `NpmPackage`) when resolution fails.
#[must_use]
pub fn is_path_alias(specifier: &str) -> bool {
    // `#` prefix is Node.js imports maps (package.json "imports" field)
    if specifier.starts_with('#') {
        return true;
    }
    // `~/`, `~~/`, and `@@/` prefixes are common alias conventions
    // (e.g., Nuxt, custom tsconfig)
    if specifier.starts_with("~/") || specifier.starts_with("~~/") || specifier.starts_with("@@/") {
        return true;
    }
    // `@/` is a very common path alias (e.g., `@/components/Foo`)
    if specifier.starts_with("@/") {
        return true;
    }
    // npm scoped packages MUST be lowercase (npm registry requirement).
    // PascalCase `@Scope` or `@Scope/path` patterns are tsconfig path aliases,
    // not npm packages. E.g., `@Components`, `@Hooks/useApi`, `@Services/auth`.
    if specifier.starts_with('@') {
        let scope = specifier.split('/').next().unwrap_or(specifier);
        if scope.len() > 1 && scope.chars().nth(1).is_some_and(|c| c.is_ascii_uppercase()) {
            return true;
        }
    }

    false
}

/// Check if a specifier is a bare specifier (npm package or Node.js imports map entry).
#[must_use]
pub fn is_bare_specifier(specifier: &str) -> bool {
    !specifier.starts_with('.')
        && !specifier.starts_with('/')
        && !specifier.contains("://")
        && !specifier.starts_with("data:")
}

/// Check if a string looks like a valid npm package name.
///
/// Rejects strings that are clearly not packages: shell variables (`$X`),
/// pure numbers, strings starting with `!`, empty strings, etc.
/// This prevents false "unlisted dependency" reports for test fixture
/// artifacts like `$DIR` or `1`.
#[must_use]
pub fn is_valid_package_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.as_bytes()[0];
    // Reject shell variables, shebangs, and similar non-package prefixes
    if first == b'$' || first == b'!' || first == b'#' {
        return false;
    }
    // Reject bundler-internal specifiers (webpack loaders, turbopack barrel optimization)
    if name.contains('?') || name.contains('!') || name.starts_with("__") {
        return false;
    }
    // Pure numeric strings (like "1", "123") are not package names
    if name.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    // Must contain at least one letter or @ sign to be a plausible package name
    if !name.bytes().any(|b| b.is_ascii_alphabetic() || b == b'@') {
        return false;
    }
    // Reject strings with spaces or backslashes (not valid in npm names)
    !name.contains(' ') && !name.contains('\\')
}

/// Extract the npm package name from a specifier.
/// `@scope/pkg/foo/bar` -> `@scope/pkg`
/// `lodash/merge` -> `lodash`
#[must_use]
pub fn extract_package_name(specifier: &str) -> String {
    if specifier.starts_with('@') {
        let parts: Vec<&str> = specifier.splitn(3, '/').collect();
        if parts.len() >= 2 {
            format!("{}/{}", parts[0], parts[1])
        } else {
            specifier.to_string()
        }
    } else {
        specifier.split('/').next().unwrap_or(specifier).to_string()
    }
}

/// Normalize the body of a Deno `npm:` specifier into a plain npm package
/// reference by dropping the `@<version>` selector while preserving the package
/// name (including scope) and any subpath.
///
/// `rest` is the substring after the `npm:` scheme. Deno `npm:` specifiers are
/// `<package>[@<version>][/<subpath>]`, so the version selector is the first
/// `@` that follows the package name (after the scope's own `@scope/` for
/// scoped packages). Examples:
/// `@supabase/supabase-js@2` -> `@supabase/supabase-js`,
/// `preact@10/hooks` -> `preact/hooks`, `express@^4.18.0` -> `express`,
/// `foo` -> `foo`.
#[must_use]
pub fn normalize_npm_specifier(rest: &str) -> String {
    // For scoped packages, skip past the `@scope/` segment so the leading scope
    // `@` is not mistaken for the version selector.
    let search_from = if rest.starts_with('@') {
        match rest.find('/') {
            Some(slash) => slash + 1,
            // `@scope` alone carries no version or subpath.
            None => return rest.to_string(),
        }
    } else {
        0
    };

    let Some(at_rel) = rest[search_from..].find('@') else {
        return rest.to_string();
    };
    let at = search_from + at_rel;
    // The version runs from the `@` until the next `/` (a subpath) or the end.
    let end = rest[at..].find('/').map_or(rest.len(), |slash| at + slash);
    let mut out = String::with_capacity(rest.len() - (end - at));
    out.push_str(&rest[..at]);
    out.push_str(&rest[end..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_package_name() {
        assert_eq!(extract_package_name("react"), "react");
        assert_eq!(extract_package_name("lodash/merge"), "lodash");
        assert_eq!(extract_package_name("@scope/pkg"), "@scope/pkg");
        assert_eq!(extract_package_name("@scope/pkg/foo"), "@scope/pkg");
    }

    #[test]
    fn normalize_npm_specifier_scoped_with_version() {
        assert_eq!(
            normalize_npm_specifier("@supabase/supabase-js@2"),
            "@supabase/supabase-js"
        );
    }

    #[test]
    fn normalize_npm_specifier_unscoped_with_version() {
        assert_eq!(normalize_npm_specifier("express@^4.18.0"), "express");
    }

    #[test]
    fn normalize_npm_specifier_version_then_subpath() {
        // Deno places the version before the subpath: `<pkg>@<ver>/<subpath>`.
        assert_eq!(normalize_npm_specifier("preact@10/hooks"), "preact/hooks");
        assert_eq!(
            normalize_npm_specifier("@scope/name@1.2.3/sub"),
            "@scope/name/sub"
        );
    }

    #[test]
    fn normalize_npm_specifier_no_version() {
        assert_eq!(normalize_npm_specifier("foo"), "foo");
        assert_eq!(normalize_npm_specifier("lodash/merge"), "lodash/merge");
        assert_eq!(normalize_npm_specifier("@scope/pkg/sub"), "@scope/pkg/sub");
    }

    #[test]
    fn normalize_npm_specifier_scope_only() {
        // A bare scope with no `/name` is degenerate but must not panic.
        assert_eq!(normalize_npm_specifier("@scope"), "@scope");
    }

    #[test]
    fn normalize_npm_specifier_empty() {
        // A bare `npm:` body normalizes to empty; the resolver maps this to an
        // external file rather than emitting a finding for the empty specifier.
        assert_eq!(normalize_npm_specifier(""), "");
    }

    #[test]
    fn test_is_bare_specifier() {
        assert!(is_bare_specifier("react"));
        assert!(is_bare_specifier("@scope/pkg"));
        assert!(is_bare_specifier("#internal/module"));
        assert!(!is_bare_specifier("./utils"));
        assert!(!is_bare_specifier("../lib"));
        assert!(!is_bare_specifier("/absolute"));
    }

    #[test]
    fn test_is_bare_specifier_url_specifiers() {
        assert!(!is_bare_specifier("https://cdn.example.com/lib.js"));
        assert!(!is_bare_specifier("http://example.com/module"));
        assert!(!is_bare_specifier("data:text/javascript,export default 42"));
    }

    // ── is_path_alias ───────────────────────────────────────────────

    #[test]
    fn path_alias_hash_prefix() {
        assert!(is_path_alias("#internal/module"));
        assert!(is_path_alias("#shared"));
    }

    #[test]
    fn path_alias_tilde_prefix() {
        assert!(is_path_alias("~/components/Button"));
        assert!(is_path_alias("~~/utils/helpers"));
        assert!(is_path_alias("@@/shared/utils"));
    }

    #[test]
    fn path_alias_at_slash_prefix() {
        assert!(is_path_alias("@/components/Button"));
        assert!(is_path_alias("@/lib"));
    }

    #[test]
    fn path_alias_pascal_case_scope() {
        // PascalCase scoped packages are tsconfig aliases, not npm packages
        assert!(is_path_alias("@Components/Button"));
        assert!(is_path_alias("@Hooks/useApi"));
        assert!(is_path_alias("@Services/auth"));
    }

    #[test]
    fn path_alias_lowercase_scope_is_not_alias() {
        // Lowercase scoped packages are regular npm packages
        assert!(!is_path_alias("@babel/core"));
        assert!(!is_path_alias("@types/react"));
        assert!(!is_path_alias("@scope/pkg"));
    }

    #[test]
    fn path_alias_plain_specifier_is_not_alias() {
        assert!(!is_path_alias("react"));
        assert!(!is_path_alias("lodash/merge"));
        assert!(!is_path_alias("my-utils"));
    }

    #[test]
    fn path_alias_tilde_without_slash_is_not_alias() {
        // `~something` without a slash is not a path alias convention
        assert!(!is_path_alias("~something"));
    }

    // ── is_valid_package_name ────────────────────────────────────────

    #[test]
    fn valid_package_names() {
        assert!(is_valid_package_name("react"));
        assert!(is_valid_package_name("@scope/pkg"));
        assert!(is_valid_package_name("lodash.get"));
        assert!(is_valid_package_name("my-pkg"));
        assert!(is_valid_package_name("@babel/core"));
        assert!(is_valid_package_name("3d-view")); // starts with digit but has letters
    }

    #[test]
    fn invalid_package_names() {
        assert!(!is_valid_package_name("$DIR"));
        assert!(!is_valid_package_name("$ENV_VAR"));
        assert!(!is_valid_package_name("1"));
        assert!(!is_valid_package_name("123"));
        assert!(!is_valid_package_name(""));
        assert!(!is_valid_package_name("!important"));
        assert!(!is_valid_package_name("has spaces"));
        assert!(!is_valid_package_name("back\\slash"));
    }

    // ── extract_package_name edge cases ─────────────────────────────

    #[test]
    fn extract_package_name_bare_scope_only() {
        // Edge case: just `@scope` without a package name
        assert_eq!(extract_package_name("@scope"), "@scope");
    }

    #[test]
    fn extract_package_name_deep_subpath() {
        assert_eq!(
            extract_package_name("@scope/pkg/deep/nested/path"),
            "@scope/pkg"
        );
    }

    #[test]
    fn extract_package_name_single_name() {
        assert_eq!(extract_package_name("react"), "react");
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// Any specifier starting with `.` or `/` must NOT be classified as a bare specifier.
            #[test]
            fn relative_paths_are_not_bare(suffix in "[a-zA-Z0-9_/.-]{0,80}") {
                let dot = format!(".{suffix}");
                let slash = format!("/{suffix}");
                prop_assert!(!is_bare_specifier(&dot), "'.{suffix}' was classified as bare");
                prop_assert!(!is_bare_specifier(&slash), "'/{suffix}' was classified as bare");
            }

            /// Scoped packages (@scope/pkg) should extract exactly `@scope/pkg` — two segments.
            #[test]
            fn scoped_package_name_has_two_segments(
                scope in "[a-z][a-z0-9-]{0,20}",
                pkg in "[a-z][a-z0-9-]{0,20}",
                subpath in "(/[a-z0-9-]{1,20}){0,3}",
            ) {
                let specifier = format!("@{scope}/{pkg}{subpath}");
                let extracted = extract_package_name(&specifier);
                let expected = format!("@{scope}/{pkg}");
                prop_assert_eq!(extracted, expected);
            }

            /// Unscoped packages should extract exactly the first path segment.
            #[test]
            fn unscoped_package_name_is_first_segment(
                pkg in "[a-z][a-z0-9-]{0,30}",
                subpath in "(/[a-z0-9-]{1,20}){0,3}",
            ) {
                let specifier = format!("{pkg}{subpath}");
                let extracted = extract_package_name(&specifier);
                prop_assert_eq!(extracted, pkg);
            }

            /// is_bare_specifier, is_path_alias, and is_valid_package_name should never panic on arbitrary strings.
            #[test]
            fn classification_functions_no_panic(s in "[a-zA-Z0-9@#~/._$!\\-]{1,100}") {
                let _ = is_bare_specifier(&s);
                let _ = is_path_alias(&s);
                let _ = is_valid_package_name(&s);
            }

            /// Valid npm package names (lowercase letters, digits, hyphens, dots) must be accepted.
            #[test]
            fn valid_npm_names_accepted(name in "[a-z][a-z0-9._-]{0,30}") {
                prop_assert!(is_valid_package_name(&name));
            }

            /// Shell variable specifiers ($...) must be rejected.
            #[test]
            fn shell_variables_rejected(suffix in "[A-Z_]{1,20}") {
                let specifier = format!("${suffix}");
                prop_assert!(!is_valid_package_name(&specifier));
            }

            /// Pure numeric specifiers must be rejected.
            #[test]
            fn pure_numbers_rejected(n in "[0-9]{1,10}") {
                prop_assert!(!is_valid_package_name(&n));
            }

            /// `@/` prefix should always be detected as a path alias.
            #[test]
            fn at_slash_is_path_alias(suffix in "[a-zA-Z0-9_/.-]{0,80}") {
                let specifier = format!("@/{suffix}");
                prop_assert!(is_path_alias(&specifier));
            }

            /// `~/` prefix should always be detected as a path alias.
            #[test]
            fn tilde_slash_is_path_alias(suffix in "[a-zA-Z0-9_/.-]{0,80}") {
                let specifier = format!("~/{suffix}");
                prop_assert!(is_path_alias(&specifier));
            }

            /// `#` prefix should always be detected as a path alias (Node.js imports map).
            #[test]
            fn hash_prefix_is_path_alias(suffix in "[a-zA-Z0-9_/.-]{0,80}") {
                let specifier = format!("#{suffix}");
                prop_assert!(is_path_alias(&specifier));
            }

            /// Extracted package name from node_modules path should never be empty.
            #[test]
            fn node_modules_package_name_never_empty(
                pkg in "[a-z][a-z0-9-]{0,20}",
                file in "[a-z]{1,10}\\.(js|ts|mjs)",
            ) {
                let path = std::path::PathBuf::from(format!("/project/node_modules/{pkg}/{file}"));
                if let Some(name) = crate::resolve::fallbacks::extract_package_name_from_node_modules_path(&path) {
                    prop_assert!(!name.is_empty());
                }
            }
        }
    }
}
