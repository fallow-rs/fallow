/// Extract file path references from a package.json script value.
///
/// Recognises patterns like:
/// - `node path/to/script.js`
/// - `ts-node path/to/script.ts`
/// - `tsx path/to/script.ts`
/// - `npx ts-node path/to/script.ts`
/// - Bare file paths ending in `.js`, `.ts`, `.mjs`, `.cjs`, `.mts`, `.cts`
///
/// Script values are split by `&&`, `||`, and `;` to handle chained commands.
pub fn extract_script_file_refs(script: &str) -> Vec<String> {
    let mut refs = Vec::new();

    // Runners whose next argument is a file path
    const RUNNERS: &[&str] = &["node", "ts-node", "tsx", "babel-node"];

    // Split on shell operators to handle chained commands
    for segment in script.split(&['&', '|', ';'][..]) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }

        let tokens: Vec<&str> = segment.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        // Skip leading `npx`/`pnpx`/`yarn`/`pnpm exec` to find the actual command
        let mut start = 0;
        if matches!(tokens.first(), Some(&"npx" | &"pnpx")) {
            start = 1;
        } else if tokens.len() >= 2 && matches!(tokens[0], "yarn" | "pnpm") && tokens[1] == "exec" {
            start = 2;
        }

        if start >= tokens.len() {
            continue;
        }

        let cmd = tokens[start];

        // Check if the command is a known runner
        if RUNNERS.contains(&cmd) {
            // Collect ALL file path arguments after the runner (handles
            // `node --test file1.mjs file2.mjs ...` and similar multi-file patterns)
            for &token in &tokens[start + 1..] {
                if token.starts_with('-') {
                    continue;
                }
                // Must look like a file path (contains '/' or '.' extension)
                if looks_like_file_path(token) {
                    refs.push(token.to_string());
                }
            }
        } else {
            // Scan all tokens for bare file paths (e.g. `./scripts/build.js`)
            for &token in &tokens[start..] {
                if token.starts_with('-') {
                    continue;
                }
                if looks_like_script_file(token) {
                    refs.push(token.to_string());
                }
            }
        }
    }

    refs
}

/// Check if a token looks like a file path argument (has a directory separator or a
/// JS/TS file extension).
pub fn looks_like_file_path(token: &str) -> bool {
    if !crate::scripts::could_be_file_path(token) {
        return false;
    }
    let extensions = [".js", ".ts", ".mjs", ".cjs", ".mts", ".cts", ".jsx", ".tsx"];
    if extensions.iter().any(|ext| token.ends_with(ext)) {
        return true;
    }
    // Only treat tokens with `/` as paths if they look like actual file paths,
    // not URLs or scoped package names like @scope/package
    token.starts_with("./")
        || token.starts_with("../")
        || (token.contains('/') && !token.starts_with('@') && !token.contains("://"))
}

/// Check if a token looks like a standalone script file reference (must have a
/// JS/TS extension and a path-like structure, not a bare command name).
pub fn looks_like_script_file(token: &str) -> bool {
    if !crate::scripts::could_be_file_path(token) {
        return false;
    }
    let extensions = [".js", ".ts", ".mjs", ".cjs", ".mts", ".cts", ".jsx", ".tsx"];
    if !extensions.iter().any(|ext| token.ends_with(ext)) {
        return false;
    }
    // Must contain a path separator or start with ./ to distinguish from
    // bare package names like `webpack.js`
    token.contains('/') || token.starts_with("./") || token.starts_with("../")
}

#[cfg(test)]
mod tests {
    use super::*;

    // extract_script_file_refs tests (Issue 3)
    #[test]
    fn script_node_runner() {
        let refs = extract_script_file_refs("node utilities/generate-coverage-badge.js");
        assert_eq!(refs, vec!["utilities/generate-coverage-badge.js"]);
    }

    #[test]
    fn script_ts_node_runner() {
        let refs = extract_script_file_refs("ts-node scripts/seed.ts");
        assert_eq!(refs, vec!["scripts/seed.ts"]);
    }

    #[test]
    fn script_tsx_runner() {
        let refs = extract_script_file_refs("tsx scripts/migrate.ts");
        assert_eq!(refs, vec!["scripts/migrate.ts"]);
    }

    #[test]
    fn script_npx_prefix() {
        let refs = extract_script_file_refs("npx ts-node scripts/generate.ts");
        assert_eq!(refs, vec!["scripts/generate.ts"]);
    }

    #[test]
    fn script_chained_commands() {
        let refs = extract_script_file_refs("node scripts/build.js && node scripts/post-build.js");
        assert_eq!(refs, vec!["scripts/build.js", "scripts/post-build.js"]);
    }

    #[test]
    fn script_with_flags() {
        let refs = extract_script_file_refs(
            "node --experimental-specifier-resolution=node scripts/run.mjs",
        );
        assert_eq!(refs, vec!["scripts/run.mjs"]);
    }

    #[test]
    fn script_no_file_ref() {
        let refs = extract_script_file_refs("next build");
        assert!(refs.is_empty());
    }

    #[test]
    fn script_bare_file_path() {
        let refs = extract_script_file_refs("echo done && node ./scripts/check.js");
        assert_eq!(refs, vec!["./scripts/check.js"]);
    }

    #[test]
    fn script_semicolon_separator() {
        let refs = extract_script_file_refs("node scripts/a.js; node scripts/b.ts");
        assert_eq!(refs, vec!["scripts/a.js", "scripts/b.ts"]);
    }

    // looks_like_file_path tests
    #[test]
    fn file_path_with_extension() {
        assert!(looks_like_file_path("scripts/build.js"));
        assert!(looks_like_file_path("scripts/build.ts"));
        assert!(looks_like_file_path("scripts/build.mjs"));
    }

    #[test]
    fn file_path_with_slash() {
        assert!(looks_like_file_path("scripts/build"));
    }

    #[test]
    fn not_file_path() {
        assert!(!looks_like_file_path("--watch"));
        assert!(!looks_like_file_path("build"));
    }

    // looks_like_script_file tests
    #[test]
    fn script_file_with_path() {
        assert!(looks_like_script_file("scripts/build.js"));
        assert!(looks_like_script_file("./scripts/build.ts"));
        assert!(looks_like_script_file("../scripts/build.mjs"));
    }

    #[test]
    fn not_script_file_bare_name() {
        // Bare names without path separator should not match
        assert!(!looks_like_script_file("webpack.js"));
        assert!(!looks_like_script_file("build"));
    }

    // Negative-guard tests (shared with scripts::could_be_file_path):
    // tokens whose syntax precludes a Unix path must not be classified as
    // script file references, even when their suffix matches a known
    // extension.

    #[test]
    fn looks_like_file_path_rejects_gha_fragments() {
        assert!(!looks_like_file_path("${{ env.URL }}/api.ts"));
        assert!(!looks_like_file_path("}}/api/health.ts"));
    }

    #[test]
    fn looks_like_file_path_rejects_backslash_and_bracket_class() {
        assert!(!looks_like_file_path(r"path\to\file.ts"));
        assert!(!looks_like_file_path(".[]"));
        assert!(!looks_like_file_path("prefix/[^unclosed.ts"));
    }

    #[test]
    fn looks_like_file_path_passes_nextjs_dynamic_route() {
        assert!(looks_like_file_path("app/[id]/page.tsx"));
        assert!(looks_like_file_path("pages/[...slug].ts"));
    }

    #[test]
    fn looks_like_script_file_rejects_gha_and_regex_fragments() {
        assert!(!looks_like_script_file("${{ env.X }}/path.ts"));
        assert!(!looks_like_script_file(r"path\file.ts"));
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// looks_like_file_path should never panic on arbitrary strings.
            #[test]
            fn looks_like_file_path_no_panic(s in "[a-zA-Z0-9_./@-]{1,80}") {
                let _ = looks_like_file_path(&s);
            }

            /// looks_like_script_file should never panic on arbitrary strings.
            #[test]
            fn looks_like_script_file_no_panic(s in "[a-zA-Z0-9_./@-]{1,80}") {
                let _ = looks_like_script_file(&s);
            }

            /// extract_script_file_refs should never panic on arbitrary input.
            #[test]
            fn extract_script_file_refs_no_panic(s in "[a-zA-Z0-9 _./@&|;-]{1,200}") {
                let _ = extract_script_file_refs(&s);
            }
        }
    }
}
