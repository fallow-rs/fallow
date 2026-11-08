//! graphql-config plugin.
//!
//! GraphQL tools such as GraphQL Codegen, the GraphQL language server,
//! graphql-eslint, and code generators read a shared `graphql.config.*` or
//! `.graphqlrc*` file. Its `schema` and `documents` globs name the GraphQL
//! files that the tool reads, so no import reaches them. The plugin marks the
//! GraphQL files that those globs match as always used, also under
//! `projects.<name>`.
//!
//! The plugin activates from the `graphql-config` package or from a config
//! file anywhere in the project, because the tools that read the file often
//! depend on `graphql-config` only transitively.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fallow_config::PackageJson;
use fallow_config::yaml::{self, YamlNode};

use super::registry::ConfigCandidateIndex;
use super::{PathRule, Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["graphql-config"];

const CONFIG_PATTERNS: &[&str] = &[
    "graphql.config.{ts,cts,mts,js,cjs,mjs,json,yml,yaml,toml}",
    ".graphqlrc",
    ".graphqlrc.{ts,cts,mts,js,cjs,mjs,json,yml,yaml,toml}",
];

const ALWAYS_USED: &[&str] = &[
    "**/graphql.config.{ts,cts,mts,js,cjs,mjs,json,yml,yaml,toml}",
    "**/.graphqlrc",
    "**/.graphqlrc.{ts,cts,mts,js,cjs,mjs,json,yml,yaml,toml}",
];

/// The config file names that activate the plugin. The discovery walk
/// collects only source files and `CONFIG_PATTERNS` matches, so the wildcard
/// extension does not admit other files.
const ACTIVATION_PATTERN: &str = "{graphql.config.*,.graphqlrc,.graphqlrc.*}";

/// The literal config names that the production-mode root probe checks.
const ROOT_CONFIG_NAMES: &[&str] = &[
    "graphql.config.ts",
    "graphql.config.cts",
    "graphql.config.mts",
    "graphql.config.js",
    "graphql.config.cjs",
    "graphql.config.mjs",
    "graphql.config.json",
    "graphql.config.yml",
    "graphql.config.yaml",
    "graphql.config.toml",
    ".graphqlrc",
    ".graphqlrc.ts",
    ".graphqlrc.cts",
    ".graphqlrc.mts",
    ".graphqlrc.js",
    ".graphqlrc.cjs",
    ".graphqlrc.mjs",
    ".graphqlrc.json",
    ".graphqlrc.yml",
    ".graphqlrc.yaml",
    ".graphqlrc.toml",
];

/// The `package.json` key that holds an inline graphql-config.
const PACKAGE_JSON_KEY: &str = "graphql";

const TOOLING_DEPENDENCIES: &[&str] = &["graphql-config"];

/// The file extensions of GraphQL schema and operation files. A `schema` or
/// `documents` glob that names another extension, such as `src/**/*.tsx`,
/// gets no credit: an import reaches a source file, and an always-used credit
/// for a broad source glob hides real unused files.
const GRAPHQL_EXTENSIONS: &[&str] = &["graphql", "gql", "graphqls"];

/// The config keys that hold the file globs of one project.
const FILE_KEYS: &[&str] = &["schema", "documents"];

pub struct GraphqlConfigPlugin;

impl Plugin for GraphqlConfigPlugin {
    fn name(&self) -> &'static str {
        "graphql-config"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_files(
        &self,
        deps: &[String],
        root: &Path,
        discovered_files: &[PathBuf],
        candidate_index: Option<&ConfigCandidateIndex>,
    ) -> bool {
        if self.is_enabled_with_deps(deps, root) {
            return true;
        }
        let Some(matcher) = config_file_matcher() else {
            return false;
        };
        match candidate_index {
            Some(index) => index.any_descendant_matches(root, matcher),
            // Production mode has no candidate index, and its walk skips
            // config files, so probe the root for each config name.
            None => {
                ROOT_CONFIG_NAMES
                    .iter()
                    .any(|name| root.join(name).is_file())
                    || discovered_files.iter().any(|file| {
                        file.starts_with(root)
                            && file
                                .file_name()
                                .is_some_and(|name| matcher.is_match(Path::new(name)))
                    })
            }
        }
    }

    fn is_enabled_with_package_json(&self, _pkg: &PackageJson, root: &Path) -> bool {
        // `PackageJson` does not keep the `graphql` key, so read the manifest.
        std::fs::read_to_string(root.join("package.json"))
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok())
            .is_some_and(|manifest| {
                manifest
                    .get(PACKAGE_JSON_KEY)
                    .is_some_and(|value| value.is_object())
            })
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn package_json_config_key(&self) -> Option<&'static str> {
        Some(PACKAGE_JSON_KEY)
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        for raw in config_file_globs(config_path, source) {
            if !names_graphql_files(&raw) {
                continue;
            }
            let raw = raw.strip_prefix("./").unwrap_or(&raw);
            if let Some(pattern) = config_parser::normalize_config_path(raw, config_path, root) {
                result.always_used_files.push(pattern);
            } else if let Some(pattern) =
                config_parser::parent_relative_config_path(raw, config_path, root)
            {
                // A workspace run reads the config with the package directory
                // as its root, so a glob into a sibling directory climbs out.
                let mut rule = PathRule::new(pattern);
                rule.parent_relative = true;
                result.entry_patterns.push(rule);
            }
        }
        result
    }
}

/// The matcher for config file names, used for activation.
fn config_file_matcher() -> Option<&'static globset::GlobMatcher> {
    static MATCHER: OnceLock<Option<globset::GlobMatcher>> = OnceLock::new();
    MATCHER
        .get_or_init(|| {
            globset::Glob::new(ACTIVATION_PATTERN)
                .ok()
                .map(|glob| glob.compile_matcher())
        })
        .as_ref()
}

/// Every string in `schema` and `documents`, at the top level and in each
/// `projects.<name>` entry.
fn config_file_globs(config_path: &Path, source: &str) -> Vec<String> {
    let extension = config_path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default();
    match extension {
        "ts" | "cts" | "mts" | "js" | "cjs" | "mjs" => script_config_globs(config_path, source),
        "toml" => toml_config_globs(source),
        // A `.graphqlrc` file holds YAML or JSON, and YAML reads JSON too.
        _ => yaml_config_globs(source),
    }
}

fn script_config_globs(config_path: &Path, source: &str) -> Vec<String> {
    let mut prefixes: Vec<Vec<String>> = vec![Vec::new()];
    for project in config_parser::extract_config_object_keys(source, config_path, &["projects"]) {
        prefixes.push(vec!["projects".to_string(), project]);
    }
    let mut globs = Vec::new();
    for prefix in &prefixes {
        for key in FILE_KEYS {
            let mut prop_path: Vec<&str> = prefix.iter().map(String::as_str).collect();
            prop_path.push(key);
            globs.extend(config_parser::extract_config_string_or_array(
                source,
                config_path,
                &prop_path,
            ));
        }
    }
    globs
}

fn yaml_config_globs(source: &str) -> Vec<String> {
    let Ok(document) = yaml::parse(source) else {
        return Vec::new();
    };
    let root = document.root();
    let mut projects = vec![root];
    if let Some(mapping) = root.get("projects").and_then(YamlNode::as_mapping) {
        projects.extend(mapping.values());
    }
    let mut globs = Vec::new();
    for project in projects {
        for key in FILE_KEYS {
            let Some(value) = project.get(key) else {
                continue;
            };
            match value.as_sequence() {
                Some(items) => globs.extend(items.filter_map(YamlNode::as_str).map(str::to_string)),
                None => globs.extend(value.as_str().map(str::to_string)),
            }
        }
    }
    globs
}

fn toml_config_globs(source: &str) -> Vec<String> {
    let Ok(value) = fallow_config::toml_input::from_str::<toml::Value>(source) else {
        return Vec::new();
    };
    let mut projects = vec![&value];
    if let Some(table) = value.get("projects").and_then(toml::Value::as_table) {
        projects.extend(table.values());
    }
    let mut globs = Vec::new();
    for project in projects {
        for key in FILE_KEYS {
            match project.get(key) {
                Some(toml::Value::String(glob)) => globs.push(glob.clone()),
                Some(toml::Value::Array(items)) => globs.extend(
                    items
                        .iter()
                        .filter_map(toml::Value::as_str)
                        .map(str::to_string),
                ),
                _ => {}
            }
        }
    }
    globs
}

/// Whether `raw` is a local glob that only names GraphQL files. A URL, a
/// negated glob, and a glob for other file types return `false`.
fn names_graphql_files(raw: &str) -> bool {
    if raw.starts_with('!') || raw.contains("://") {
        return false;
    }
    let file_name = raw.rsplit('/').next().unwrap_or(raw);
    let Some((_, extension)) = file_name.rsplit_once('.') else {
        return false;
    };
    let extensions = extension
        .strip_prefix('{')
        .and_then(|inner| inner.strip_suffix('}'))
        .unwrap_or(extension);
    extensions
        .split(',')
        .all(|extension| GRAPHQL_EXTENSIONS.contains(&extension))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn always_used(config_path: &str, source: &str) -> Vec<String> {
        GraphqlConfigPlugin
            .resolve_config(Path::new(config_path), source, Path::new("/project"))
            .always_used_files
    }

    #[test]
    fn script_config_schema_and_documents_are_always_used() {
        let source = r#"
            import { defineConfig } from "some-graphql-generator/Config";
            export default defineConfig({
              schema: "./schema/github.graphql",
              documents: ["src/**/*.graphql", "!src/legacy/**"],
            });
        "#;
        assert_eq!(
            always_used("/project/packages/github/graphql.config.ts", source),
            vec![
                "packages/github/schema/github.graphql",
                "packages/github/src/**/*.graphql",
            ]
        );
    }

    #[test]
    fn project_entries_are_always_used() {
        let source = r"
            module.exports = {
              projects: {
                app: { schema: 'app/schema.graphql', documents: 'app/**/*.gql' },
                admin: { schema: ['admin/schema.graphql'] },
              },
            };
        ";
        assert_eq!(
            always_used("/project/graphql.config.cjs", source),
            vec!["app/schema.graphql", "app/**/*.gql", "admin/schema.graphql"]
        );
    }

    #[test]
    fn yaml_config_reads_projects_and_skips_urls() {
        let source = "schema: https://api.example.com/graphql\n\
                      documents: ops/*.graphql\n\
                      projects:\n  web:\n    schema:\n      - web/schema.graphql\n";
        assert_eq!(
            always_used("/project/.graphqlrc.yml", source),
            vec!["ops/*.graphql", "web/schema.graphql"]
        );
    }

    #[test]
    fn json_graphqlrc_without_extension_is_read() {
        let source = r#"{ "schema": "schema.graphql", "documents": ["src/**/*.{graphql,gql}"] }"#;
        assert_eq!(
            always_used("/project/.graphqlrc", source),
            vec!["schema.graphql", "src/**/*.{graphql,gql}"]
        );
    }

    #[test]
    fn toml_config_is_read() {
        let source =
            "schema = \"schema.graphql\"\n[projects.web]\ndocuments = [\"web/**/*.graphql\"]\n";
        assert_eq!(
            always_used("/project/graphql.config.toml", source),
            vec!["schema.graphql", "web/**/*.graphql"]
        );
    }

    #[test]
    fn source_file_globs_get_no_credit() {
        let source = "documents: ['src/**/*.tsx', 'src/**/*.{ts,graphql}', 'schema.ts']\n";
        assert!(always_used("/project/.graphqlrc.yaml", source).is_empty());
    }

    #[test]
    fn config_file_names_activate_the_plugin() {
        let matcher = config_file_matcher().unwrap();
        for name in [
            "graphql.config.ts",
            "graphql.config.yml",
            ".graphqlrc",
            ".graphqlrc.json",
        ] {
            assert!(matcher.is_match(name), "{name}");
        }
        assert!(!matcher.is_match("graphql.ts"));
        assert!(!matcher.is_match("my.graphqlrc"));
        assert!(!matcher.is_match("codegen.ts"));
    }

    #[test]
    fn glob_above_the_root_is_a_parent_relative_entry() {
        let result = GraphqlConfigPlugin.resolve_config(
            Path::new("/project/packages/api/graphql.config.ts"),
            "export default { documents: ['../shared/*.graphql'] };",
            Path::new("/project/packages/api"),
        );
        assert!(result.always_used_files.is_empty());
        let rules: Vec<(&str, bool)> = result
            .entry_patterns
            .iter()
            .map(|rule| (rule.pattern.as_str(), rule.parent_relative))
            .collect();
        assert_eq!(rules, vec![("../shared/*.graphql", true)]);
    }

    #[test]
    fn package_json_graphql_key_activates() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{ "name": "app", "graphql": { "schema": "schema.graphql" } }"#,
        )
        .unwrap();
        assert!(
            GraphqlConfigPlugin.is_enabled_with_package_json(&PackageJson::default(), dir.path())
        );

        std::fs::write(dir.path().join("package.json"), r#"{ "name": "app" }"#).unwrap();
        assert!(
            !GraphqlConfigPlugin.is_enabled_with_package_json(&PackageJson::default(), dir.path())
        );
    }

    #[test]
    fn root_config_activates_without_candidate_index() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".graphqlrc.yml"),
            "schema: schema.graphql\n",
        )
        .unwrap();
        assert!(GraphqlConfigPlugin.is_enabled_with_files(&[], dir.path(), &[], None));
    }

    #[test]
    fn config_file_in_a_package_activates_without_dependency() {
        let files = vec![PathBuf::from("/project/packages/api/graphql.config.ts")];
        assert!(GraphqlConfigPlugin.is_enabled_with_files(
            &[],
            Path::new("/project"),
            &files,
            None
        ));
        assert!(!GraphqlConfigPlugin.is_enabled_with_files(
            &[],
            Path::new("/project"),
            &[PathBuf::from("/project/src/index.ts")],
            None
        ));
    }
}
