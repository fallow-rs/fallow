//! Prisma ORM plugin.
//!
//! Detects Prisma projects, marks seed files as entry points, marks schema/
//! config files as always used, and credits npm packages referenced as custom
//! `generator` providers inside `schema.prisma` so they are not reported as
//! `unused-dependency`.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["prisma", "@prisma/client"];

const ENTRY_PATTERNS: &[&str] = &["prisma/seed.{ts,js}"];

const CONFIG_PATTERNS: &[&str] = &[
    "prisma.config.{ts,mts,cts,js,mjs,cjs}",
    ".config/prisma.{ts,mts,cts,js,mjs,cjs}",
    "prisma/schema.prisma",
    "schema.prisma",
    "prisma/schema/*.prisma",
];

const ALWAYS_USED: &[&str] = &[
    "prisma/schema.prisma",
    "schema.prisma",
    "prisma/schema/*.prisma",
    "prisma.config.{ts,mts,cts,js,mjs,cjs}",
    ".config/prisma.{ts,mts,cts,js,mjs,cjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["prisma", "@prisma/client"];

define_plugin! {
    struct PrismaPlugin => "prisma",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        if config_path.extension().is_some_and(|ext| ext == "prisma") {
            result
                .referenced_dependencies
                .extend(parse_generator_providers(source));
        } else if is_prisma_config_path(config_path)
            && let Some(schema) = config_parser::extract_config_string(source, config_path, &["schema"])
        {
            add_configured_schema(&mut result, config_path, root, &schema);
        }
        result
    }
}

fn is_prisma_config_path(path: &Path) -> bool {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == "prisma.config" || stem == "prisma")
}

fn add_configured_schema(result: &mut PluginResult, config_path: &Path, root: &Path, schema: &str) {
    let Some(normalized) = config_parser::normalize_config_path(schema, config_path, root) else {
        return;
    };
    let absolute = root.join(&normalized);

    if is_schema_file_path(&absolute) {
        result.always_used_files.push(normalized);
        result
            .referenced_dependencies
            .extend(read_schema_provider_dependencies(&absolute));
        return;
    }

    result
        .always_used_files
        .push(format!("{}/**/*.prisma", normalized.trim_end_matches('/')));
    result
        .referenced_dependencies
        .extend(read_schema_folder_provider_dependencies(&absolute));
}

fn is_schema_file_path(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "prisma") || path.is_file()
}

fn read_schema_provider_dependencies(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|source| parse_generator_providers(&source))
        .unwrap_or_default()
}

fn read_schema_folder_provider_dependencies(path: &Path) -> Vec<String> {
    let mut providers = Vec::new();
    collect_schema_folder_provider_dependencies(path, &mut providers);
    providers.sort();
    providers.dedup();
    providers
}

fn collect_schema_folder_provider_dependencies(path: &Path, providers: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let child = entry.path();
        if child.is_dir() {
            collect_schema_folder_provider_dependencies(&child, providers);
        } else if child.extension().is_some_and(|ext| ext == "prisma") {
            providers.extend(read_schema_provider_dependencies(&child));
        }
    }
}

#[expect(
    clippy::expect_used,
    reason = "static Prisma regex patterns are hard-coded and covered by plugin tests"
)]
static GENERATOR_BLOCK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\bgenerator\s+\w+\s*\{([^}]*)\}").expect("valid regex"));

#[expect(
    clippy::expect_used,
    reason = "static Prisma regex patterns are hard-coded and covered by plugin tests"
)]
static PROVIDER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^\s*provider\s*=\s*"([^"]+)""#).expect("valid regex"));

/// Extract npm package names referenced as `provider = "..."` inside
/// `generator <name> { ... }` blocks. `datasource` blocks are ignored since
/// their providers (`postgresql`, `mysql`, etc.) are not npm packages.
///
/// Skips the documented Prisma escape hatches that are not package names:
/// shell-command form (`provider = "node ./gen.js"`) and path form
/// (`provider = "./gen.js"`). Built-in providers like `prisma-client-js` are
/// returned verbatim; if they are not in `package.json` the unused-dep check
/// naturally never fires on them, so a denylist would be dead defensive code.
fn parse_generator_providers(source: &str) -> Vec<String> {
    let mut providers = Vec::new();
    let source = strip_schema_comments(source);
    for cap in GENERATOR_BLOCK_RE.captures_iter(&source) {
        let block = &cap[1];
        for pcap in PROVIDER_RE.captures_iter(block) {
            let value = pcap[1].trim();
            if value.is_empty() || value.contains(' ') || value.starts_with('.') {
                continue;
            }
            providers.push(value.to_owned());
        }
    }
    providers
}

fn strip_schema_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
            continue;
        }

        if ch != '/' {
            out.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some('/') => {
                chars.next();
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            Some('*') => {
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                    }
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            _ => out.push(ch),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn deps_for(source: &str) -> Vec<String> {
        let plugin = PrismaPlugin;
        let result = plugin.resolve_config(
            Path::new("prisma/schema.prisma"),
            source,
            Path::new("/project"),
        );
        result.referenced_dependencies
    }

    #[test]
    fn credits_custom_generator_provider() {
        let source = r#"
generator client {
  provider = "prisma-client-js"
}

generator json {
  provider = "prisma-json-types-generator"
}
"#;
        let deps = deps_for(source);
        assert!(deps.contains(&"prisma-json-types-generator".to_owned()));
    }

    #[test]
    fn credits_scoped_generator_provider() {
        let source = r#"
generator types {
  provider = "@prisma-community/prisma-types-generator"
}
"#;
        let deps = deps_for(source);
        assert_eq!(deps, vec!["@prisma-community/prisma-types-generator"]);
    }

    #[test]
    fn ignores_datasource_provider() {
        let source = r#"
datasource db {
  provider = "postgresql"
  url      = env("DATABASE_URL")
}
"#;
        assert!(deps_for(source).is_empty());
    }

    #[test]
    fn ignores_shell_command_provider() {
        let source = r#"
generator custom {
  provider = "node ./scripts/gen.mjs"
}
"#;
        assert!(deps_for(source).is_empty());
    }

    #[test]
    fn ignores_relative_path_provider() {
        let source = r#"
generator custom {
  provider = "./local-generator"
}
"#;
        assert!(deps_for(source).is_empty());
    }

    #[test]
    fn ignores_commented_out_provider() {
        let source = r#"
generator client {
  // provider = "prisma-erd-generator"
  provider = "prisma-client-js"
}
"#;
        let deps = deps_for(source);
        assert!(!deps.contains(&"prisma-erd-generator".to_owned()));
    }

    #[test]
    fn ignores_block_commented_out_generator() {
        let source = r#"
generator client {
  provider = "prisma-client-js"
}

/*
generator erd {
  provider = "prisma-erd-generator"
}
*/
"#;
        let deps = deps_for(source);
        assert!(!deps.contains(&"prisma-erd-generator".to_owned()));
    }

    #[test]
    fn ignores_block_commented_out_provider() {
        let source = r#"
generator erd {
  /*
  provider = "prisma-erd-generator"
  */
  provider = "prisma-client-js"
}
"#;
        let deps = deps_for(source);
        assert!(!deps.contains(&"prisma-erd-generator".to_owned()));
    }

    #[test]
    fn comment_markers_inside_strings_are_preserved() {
        let source = r#"
generator custom {
  provider = "./local//generator"
}

generator erd {
  provider = "prisma-erd-generator"
}
"#;
        assert_eq!(deps_for(source), vec!["prisma-erd-generator"]);
    }

    #[test]
    fn handles_same_line_block() {
        let source = r#"generator x { provider = "prisma-json-types-generator" }"#;
        assert_eq!(deps_for(source), vec!["prisma-json-types-generator"]);
    }

    #[test]
    fn trims_whitespace_in_provider_value() {
        let source = r#"
generator x {
  provider = "  prisma-json-types-generator  "
}
"#;
        assert_eq!(deps_for(source), vec!["prisma-json-types-generator"]);
    }

    #[test]
    fn ignores_provider_outside_any_block() {
        let source = r#"provider = "prisma-stray-generator""#;
        assert!(deps_for(source).is_empty());
    }

    #[test]
    fn empty_or_malformed_input_does_not_panic() {
        assert!(deps_for("").is_empty());
        assert!(deps_for("not a schema").is_empty());
        assert!(deps_for("generator { broken").is_empty());
    }

    #[test]
    fn non_prisma_path_returns_empty() {
        let plugin = PrismaPlugin;
        let result = plugin.resolve_config(
            Path::new("other.config.ts"),
            r#"generator x { provider = "should-not-fire" }"#,
            Path::new("/project"),
        );
        assert!(result.referenced_dependencies.is_empty());
    }

    #[test]
    fn config_patterns_include_dot_config_location() {
        let plugin = PrismaPlugin;
        assert!(
            plugin
                .config_patterns()
                .contains(&".config/prisma.{ts,mts,cts,js,mjs,cjs}")
        );
        assert!(
            plugin
                .always_used()
                .contains(&".config/prisma.{ts,mts,cts,js,mjs,cjs}")
        );
    }

    #[test]
    fn resolve_config_schema_file_marks_schema_and_reads_generator() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path();
        std::fs::create_dir_all(root.join("db")).expect("db dir");
        std::fs::write(
            root.join("db/schema.prisma"),
            r#"generator json {
  provider = "prisma-json-types-generator"
}
"#,
        )
        .expect("schema");

        let plugin = PrismaPlugin;
        let result = plugin.resolve_config(
            &root.join(".config/prisma.ts"),
            r#"export default { schema: "../db/schema.prisma" }"#,
            root,
        );

        assert_eq!(result.always_used_files, vec!["db/schema.prisma"]);
        assert_eq!(
            result.referenced_dependencies,
            vec!["prisma-json-types-generator"]
        );
    }

    #[test]
    fn resolve_config_schema_folder_marks_recursive_glob_and_reads_generators() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path();
        std::fs::create_dir_all(root.join("db/schema/nested")).expect("schema dir");
        std::fs::write(
            root.join("db/schema/generator.prisma"),
            r#"generator json {
  provider = "prisma-json-types-generator"
}
"#,
        )
        .expect("generator schema");
        std::fs::write(
            root.join("db/schema/nested/erd.prisma"),
            r#"generator erd {
  provider = "prisma-erd-generator"
}
"#,
        )
        .expect("nested schema");

        let plugin = PrismaPlugin;
        let result = plugin.resolve_config(
            &root.join(".config/prisma.ts"),
            r#"export default { schema: "../db/schema" }"#,
            root,
        );

        assert_eq!(result.always_used_files, vec!["db/schema/**/*.prisma"]);
        assert_eq!(
            result.referenced_dependencies,
            vec!["prisma-erd-generator", "prisma-json-types-generator"]
        );
    }

    #[test]
    fn multiple_generators_yield_multiple_credits() {
        let source = r#"
generator client {
  provider = "prisma-client-js"
}
generator types {
  provider = "prisma-json-types-generator"
}
generator erd {
  provider = "prisma-erd-generator"
}
"#;
        let deps = deps_for(source);
        assert!(deps.contains(&"prisma-json-types-generator".to_owned()));
        assert!(deps.contains(&"prisma-erd-generator".to_owned()));
    }
}
