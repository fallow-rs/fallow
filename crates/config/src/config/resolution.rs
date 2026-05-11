use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::boundaries::ResolvedBoundaryConfig;
use super::duplicates_config::DuplicatesConfig;
use super::flags::FlagsConfig;
use super::format::OutputFormat;
use super::health::HealthConfig;
use super::resolve::ResolveConfig;
use super::rules::{PartialRulesConfig, RulesConfig, Severity};
use super::used_class_members::UsedClassMemberRule;
use crate::external_plugin::{ExternalPluginDef, discover_external_plugins};

use super::FallowConfig;
use super::IgnoreExportsUsedInFileConfig;

/// Rule for ignoring specific exports.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct IgnoreExportRule {
    /// Glob pattern for files.
    pub file: String,
    /// Export names to ignore (`*` for all).
    pub exports: Vec<String>,
}

/// Per-file override entry.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOverride {
    /// Glob patterns to match files against (relative to config file location).
    pub files: Vec<String>,
    /// Partial rules — only specified fields override the base rules.
    #[serde(default)]
    pub rules: PartialRulesConfig,
}

/// Resolved override with pre-compiled glob matchers.
#[derive(Debug)]
pub struct ResolvedOverride {
    pub matchers: Vec<globset::GlobMatcher>,
    pub rules: PartialRulesConfig,
}

/// Fully resolved configuration with all globs pre-compiled.
#[derive(Debug)]
pub struct ResolvedConfig {
    pub root: PathBuf,
    pub entry_patterns: Vec<String>,
    pub ignore_patterns: GlobSet,
    pub output: OutputFormat,
    pub cache_dir: PathBuf,
    pub threads: usize,
    pub no_cache: bool,
    pub ignore_dependencies: Vec<String>,
    pub ignore_export_rules: Vec<IgnoreExportRule>,
    /// Whether same-file references should suppress unused-export findings.
    pub ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig,
    /// Class member names that should never be flagged as unused-class-members.
    /// Union of top-level config and active plugin contributions; merged during
    /// config resolution so analysis code reads a single list.
    pub used_class_members: Vec<UsedClassMemberRule>,
    pub duplicates: DuplicatesConfig,
    pub health: HealthConfig,
    pub rules: RulesConfig,
    /// Resolved architecture boundary configuration with pre-compiled glob matchers.
    pub boundaries: ResolvedBoundaryConfig,
    /// Whether production mode is active.
    pub production: bool,
    /// Suppress progress output and non-essential stderr messages.
    pub quiet: bool,
    /// External plugin definitions (from plugin files + inline framework definitions).
    pub external_plugins: Vec<ExternalPluginDef>,
    /// Glob patterns for dynamically loaded files (treated as always-used).
    pub dynamically_loaded: Vec<String>,
    /// Per-file rule overrides with pre-compiled glob matchers.
    pub overrides: Vec<ResolvedOverride>,
    /// Regression config (passed through from user config, not resolved).
    pub regression: Option<super::RegressionConfig>,
    /// Audit baseline paths (passed through from user config, not resolved).
    pub audit: super::AuditConfig,
    /// Optional CODEOWNERS file path (passed through for `--group-by owner`).
    pub codeowners: Option<String>,
    /// Workspace package name patterns that are public libraries.
    /// Exported API surface from these packages is not flagged as unused.
    pub public_packages: Vec<String>,
    /// Feature flag detection configuration.
    pub flags: FlagsConfig,
    /// Module resolver configuration (user-supplied import/export conditions).
    pub resolve: ResolveConfig,
    /// When true, entry file exports are subject to unused-export detection
    /// instead of being automatically marked as used. Set via the global CLI flag
    /// `--include-entry-exports` or via `includeEntryExports: true` in the fallow
    /// config file; the CLI flag ORs with the config value (CLI wins when set).
    pub include_entry_exports: bool,
}

impl FallowConfig {
    /// Resolve into a fully resolved config with compiled globs.
    pub fn resolve(
        self,
        root: PathBuf,
        output: OutputFormat,
        threads: usize,
        no_cache: bool,
        quiet: bool,
    ) -> ResolvedConfig {
        let mut ignore_builder = GlobSetBuilder::new();
        for pattern in &self.ignore_patterns {
            match Glob::new(pattern) {
                Ok(glob) => {
                    ignore_builder.add(glob);
                }
                Err(e) => {
                    tracing::warn!("invalid ignore glob pattern '{pattern}': {e}");
                }
            }
        }

        // Default ignores
        // Note: `build/` is only ignored at the project root (not `**/build/**`)
        // because nested `build/` directories like `test/build/` may contain source files.
        let default_ignores = [
            "**/node_modules/**",
            "**/dist/**",
            "build/**",
            "**/.git/**",
            "**/coverage/**",
            "**/*.min.js",
            "**/*.min.mjs",
        ];
        for pattern in &default_ignores {
            if let Ok(glob) = Glob::new(pattern) {
                ignore_builder.add(glob);
            }
        }

        let compiled_ignore_patterns = ignore_builder.build().unwrap_or_default();
        let cache_dir = root.join(".fallow");

        let mut rules = self.rules;

        // In production mode, force unused_dev_dependencies and unused_optional_dependencies off
        let production = self.production.global();
        if production {
            rules.unused_dev_dependencies = Severity::Off;
            rules.unused_optional_dependencies = Severity::Off;
        }

        let mut external_plugins = discover_external_plugins(&root, &self.plugins);
        // Merge inline framework definitions into external plugins
        external_plugins.extend(self.framework);

        // Expand boundary preset (if configured) before validation.
        // Detect source root from tsconfig.json, falling back to "src".
        let mut boundaries = self.boundaries;
        if boundaries.preset.is_some() {
            let source_root = crate::workspace::parse_tsconfig_root_dir(&root)
                .filter(|r| {
                    r != "." && !r.starts_with("..") && !std::path::Path::new(r).is_absolute()
                })
                .unwrap_or_else(|| "src".to_owned());
            if source_root != "src" {
                tracing::info!("boundary preset: using rootDir '{source_root}' from tsconfig.json");
            }
            boundaries.expand(&source_root);
        }

        // Validate and compile architecture boundary config
        let validation_errors = boundaries.validate_zone_references();
        for (rule_idx, zone_name) in &validation_errors {
            tracing::error!(
                "boundary rule {} references undefined zone '{zone_name}'",
                rule_idx
            );
        }
        for message in boundaries.validate_root_prefixes() {
            tracing::error!("{message}");
        }
        let boundaries = boundaries.resolve();

        // Pre-compile override glob matchers
        let overrides = self
            .overrides
            .into_iter()
            .filter_map(|o| {
                // Inter-file rules group findings across multiple files (a
                // single duplicate-exports finding spans N files; a single
                // circular-dependency finding spans M files in a cycle), so a
                // per-file `overrides.rules` setting cannot meaningfully turn
                // them off: the override only fires when the path being looked
                // up matches, but the finding belongs to a group of paths, not
                // to one. Warn at load time and point users at the working
                // escape hatch (`ignoreExports` for duplicates, file-level
                // `// fallow-ignore-file circular-dependency` for cycles).
                if o.rules.duplicate_exports.is_some() {
                    let files = o.files.join(", ");
                    tracing::warn!(
                        "overrides.rules.duplicate-exports has no effect for files matching [{files}]: duplicate-exports is an inter-file rule. Use top-level `ignoreExports` to exclude these files from duplicate-export grouping."
                    );
                }
                if o.rules.circular_dependencies.is_some() {
                    let files = o.files.join(", ");
                    tracing::warn!(
                        "overrides.rules.circular-dependency has no effect for files matching [{files}]: circular-dependency is an inter-file rule. Use a file-level `// fallow-ignore-file circular-dependency` comment in one participating file instead."
                    );
                }
                let matchers: Vec<globset::GlobMatcher> = o
                    .files
                    .iter()
                    .filter_map(|pattern| match Glob::new(pattern) {
                        Ok(glob) => Some(glob.compile_matcher()),
                        Err(e) => {
                            tracing::warn!("invalid override glob pattern '{pattern}': {e}");
                            None
                        }
                    })
                    .collect();
                if matchers.is_empty() {
                    None
                } else {
                    Some(ResolvedOverride {
                        matchers,
                        rules: o.rules,
                    })
                }
            })
            .collect();

        ResolvedConfig {
            root,
            entry_patterns: self.entry,
            ignore_patterns: compiled_ignore_patterns,
            output,
            cache_dir,
            threads,
            no_cache,
            ignore_dependencies: self.ignore_dependencies,
            ignore_export_rules: self.ignore_exports,
            ignore_exports_used_in_file: self.ignore_exports_used_in_file,
            used_class_members: self.used_class_members,
            duplicates: self.duplicates,
            health: self.health,
            rules,
            boundaries,
            production,
            quiet,
            external_plugins,
            dynamically_loaded: self.dynamically_loaded,
            overrides,
            regression: self.regression,
            audit: self.audit,
            codeowners: self.codeowners,
            public_packages: self.public_packages,
            flags: self.flags,
            resolve: self.resolve,
            include_entry_exports: self.include_entry_exports,
        }
    }
}

impl ResolvedConfig {
    /// Resolve the effective rules for a given file path.
    /// Starts with base rules and applies matching overrides in order.
    #[must_use]
    pub fn resolve_rules_for_path(&self, path: &Path) -> RulesConfig {
        if self.overrides.is_empty() {
            return self.rules.clone();
        }

        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        let relative_str = relative.to_string_lossy();

        let mut rules = self.rules.clone();
        for override_entry in &self.overrides {
            let matches = override_entry
                .matchers
                .iter()
                .any(|m| m.is_match(relative_str.as_ref()));
            if matches {
                rules.apply_partial(&override_entry.rules);
            }
        }
        rules
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::boundaries::BoundaryConfig;
    use crate::config::health::HealthConfig;

    #[test]
    fn overrides_deserialize() {
        let json_str = r#"{
            "overrides": [{
                "files": ["*.test.ts"],
                "rules": {
                    "unused-exports": "off"
                }
            }]
        }"#;
        let config: FallowConfig = serde_json::from_str(json_str).unwrap();
        assert_eq!(config.overrides.len(), 1);
        assert_eq!(config.overrides[0].files, vec!["*.test.ts"]);
        assert_eq!(
            config.overrides[0].rules.unused_exports,
            Some(Severity::Off)
        );
        assert_eq!(config.overrides[0].rules.unused_files, None);
    }

    #[test]
    fn resolve_rules_for_path_no_overrides() {
        let config = FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_exports: vec![],
            ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            duplicates: DuplicatesConfig::default(),
            health: HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            dynamically_loaded: vec![],
            overrides: vec![],
            regression: None,
            audit: crate::config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: FlagsConfig::default(),
            resolve: ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
        };
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        let rules = resolved.resolve_rules_for_path(Path::new("/project/src/foo.ts"));
        assert_eq!(rules.unused_files, Severity::Error);
    }

    #[test]
    fn resolve_rules_for_path_with_matching_override() {
        let config = FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_exports: vec![],
            ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            duplicates: DuplicatesConfig::default(),
            health: HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            dynamically_loaded: vec![],
            overrides: vec![ConfigOverride {
                files: vec!["*.test.ts".to_string()],
                rules: PartialRulesConfig {
                    unused_exports: Some(Severity::Off),
                    ..Default::default()
                },
            }],
            regression: None,
            audit: crate::config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: FlagsConfig::default(),
            resolve: ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
        };
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );

        // Test file matches override
        let test_rules = resolved.resolve_rules_for_path(Path::new("/project/src/utils.test.ts"));
        assert_eq!(test_rules.unused_exports, Severity::Off);
        assert_eq!(test_rules.unused_files, Severity::Error); // not overridden

        // Non-test file does not match
        let src_rules = resolved.resolve_rules_for_path(Path::new("/project/src/utils.ts"));
        assert_eq!(src_rules.unused_exports, Severity::Error);
    }

    #[test]
    fn resolve_rules_for_path_later_override_wins() {
        let config = FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_exports: vec![],
            ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            duplicates: DuplicatesConfig::default(),
            health: HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            dynamically_loaded: vec![],
            overrides: vec![
                ConfigOverride {
                    files: vec!["*.ts".to_string()],
                    rules: PartialRulesConfig {
                        unused_files: Some(Severity::Warn),
                        ..Default::default()
                    },
                },
                ConfigOverride {
                    files: vec!["*.test.ts".to_string()],
                    rules: PartialRulesConfig {
                        unused_files: Some(Severity::Off),
                        ..Default::default()
                    },
                },
            ],
            regression: None,
            audit: crate::config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: FlagsConfig::default(),
            resolve: ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
        };
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );

        // First override matches *.ts, second matches *.test.ts; second wins
        let rules = resolved.resolve_rules_for_path(Path::new("/project/foo.test.ts"));
        assert_eq!(rules.unused_files, Severity::Off);

        // Non-test .ts file only matches first override
        let rules2 = resolved.resolve_rules_for_path(Path::new("/project/foo.ts"));
        assert_eq!(rules2.unused_files, Severity::Warn);
    }

    #[test]
    fn resolve_keeps_inter_file_rule_override_after_warning() {
        // Setting `overrides.rules.duplicate-exports` for a file glob is a no-op
        // at finding-time (duplicate-exports groups span multiple files), but the
        // override must still resolve cleanly so other co-located rule settings
        // on the same override are honored. The resolver emits a tracing warning;
        // here we assert the override is still installed for non-inter-file rules.
        let config = FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_exports: vec![],
            ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            duplicates: DuplicatesConfig::default(),
            health: HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            dynamically_loaded: vec![],
            overrides: vec![ConfigOverride {
                files: vec!["**/ui/**".to_string()],
                rules: PartialRulesConfig {
                    duplicate_exports: Some(Severity::Off),
                    unused_files: Some(Severity::Warn),
                    ..Default::default()
                },
            }],
            regression: None,
            audit: crate::config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: FlagsConfig::default(),
            resolve: ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
        };
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert_eq!(
            resolved.overrides.len(),
            1,
            "inter-file rule warning must not drop the override; co-located non-inter-file rules still apply"
        );
        let rules = resolved.resolve_rules_for_path(Path::new("/project/ui/dialog.ts"));
        assert_eq!(rules.unused_files, Severity::Warn);
    }

    /// Helper to build a FallowConfig with minimal boilerplate.
    fn make_config(production: bool) -> FallowConfig {
        FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_exports: vec![],
            ignore_exports_used_in_file: IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            duplicates: DuplicatesConfig::default(),
            health: HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: BoundaryConfig::default(),
            production: production.into(),
            plugins: vec![],
            dynamically_loaded: vec![],
            overrides: vec![],
            regression: None,
            audit: crate::config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: FlagsConfig::default(),
            resolve: ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
        }
    }

    // ── Production mode ─────────────────────────────────────────────

    #[test]
    fn resolve_production_forces_dev_deps_off() {
        let resolved = make_config(true).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert_eq!(
            resolved.rules.unused_dev_dependencies,
            Severity::Off,
            "production mode should force unused_dev_dependencies to off"
        );
    }

    #[test]
    fn resolve_production_forces_optional_deps_off() {
        let resolved = make_config(true).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert_eq!(
            resolved.rules.unused_optional_dependencies,
            Severity::Off,
            "production mode should force unused_optional_dependencies to off"
        );
    }

    #[test]
    fn resolve_production_preserves_other_rules() {
        let resolved = make_config(true).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        // Other rules should remain at their defaults
        assert_eq!(resolved.rules.unused_files, Severity::Error);
        assert_eq!(resolved.rules.unused_exports, Severity::Error);
        assert_eq!(resolved.rules.unused_dependencies, Severity::Error);
    }

    #[test]
    fn resolve_non_production_keeps_dev_deps_default() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert_eq!(
            resolved.rules.unused_dev_dependencies,
            Severity::Warn,
            "non-production should keep default severity"
        );
        assert_eq!(resolved.rules.unused_optional_dependencies, Severity::Warn);
    }

    #[test]
    fn resolve_production_flag_stored() {
        let resolved = make_config(true).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(resolved.production);

        let resolved2 = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(!resolved2.production);
    }

    // ── Default ignore patterns ─────────────────────────────────────

    #[test]
    fn resolve_default_ignores_node_modules() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(
            resolved
                .ignore_patterns
                .is_match("node_modules/lodash/index.js")
        );
        assert!(
            resolved
                .ignore_patterns
                .is_match("packages/a/node_modules/react/index.js")
        );
    }

    #[test]
    fn resolve_default_ignores_dist() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(resolved.ignore_patterns.is_match("dist/bundle.js"));
        assert!(
            resolved
                .ignore_patterns
                .is_match("packages/ui/dist/index.js")
        );
    }

    #[test]
    fn resolve_default_ignores_root_build_only() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(
            resolved.ignore_patterns.is_match("build/output.js"),
            "root build/ should be ignored"
        );
        // The pattern is `build/**` (root-only), not `**/build/**`
        assert!(
            !resolved.ignore_patterns.is_match("src/build/helper.ts"),
            "nested build/ should NOT be ignored by default"
        );
    }

    #[test]
    fn resolve_default_ignores_minified_files() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(resolved.ignore_patterns.is_match("vendor/jquery.min.js"));
        assert!(resolved.ignore_patterns.is_match("lib/utils.min.mjs"));
    }

    #[test]
    fn resolve_default_ignores_git() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(resolved.ignore_patterns.is_match(".git/objects/ab/123.js"));
    }

    #[test]
    fn resolve_default_ignores_coverage() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(
            resolved
                .ignore_patterns
                .is_match("coverage/lcov-report/index.js")
        );
    }

    #[test]
    fn resolve_source_files_not_ignored_by_default() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(!resolved.ignore_patterns.is_match("src/index.ts"));
        assert!(
            !resolved
                .ignore_patterns
                .is_match("src/components/Button.tsx")
        );
        assert!(!resolved.ignore_patterns.is_match("lib/utils.js"));
    }

    // ── Custom ignore patterns ──────────────────────────────────────

    #[test]
    fn resolve_custom_ignore_patterns_merged_with_defaults() {
        let mut config = make_config(false);
        config.ignore_patterns = vec!["**/__generated__/**".to_string()];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        // Custom pattern works
        assert!(
            resolved
                .ignore_patterns
                .is_match("src/__generated__/types.ts")
        );
        // Default patterns still work
        assert!(resolved.ignore_patterns.is_match("node_modules/foo/bar.js"));
    }

    // ── Config fields passthrough ───────────────────────────────────

    #[test]
    fn resolve_passes_through_entry_patterns() {
        let mut config = make_config(false);
        config.entry = vec!["src/**/*.ts".to_string(), "lib/**/*.js".to_string()];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert_eq!(resolved.entry_patterns, vec!["src/**/*.ts", "lib/**/*.js"]);
    }

    #[test]
    fn resolve_passes_through_ignore_dependencies() {
        let mut config = make_config(false);
        config.ignore_dependencies = vec!["postcss".to_string(), "autoprefixer".to_string()];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert_eq!(
            resolved.ignore_dependencies,
            vec!["postcss", "autoprefixer"]
        );
    }

    #[test]
    fn resolve_sets_cache_dir() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/my/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert_eq!(resolved.cache_dir, PathBuf::from("/my/project/.fallow"));
    }

    #[test]
    fn resolve_passes_through_thread_count() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            8,
            true,
            true,
        );
        assert_eq!(resolved.threads, 8);
    }

    #[test]
    fn resolve_passes_through_quiet_flag() {
        let resolved = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            false,
        );
        assert!(!resolved.quiet);

        let resolved2 = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(resolved2.quiet);
    }

    #[test]
    fn resolve_passes_through_no_cache_flag() {
        let resolved_no_cache = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(resolved_no_cache.no_cache);

        let resolved_with_cache = make_config(false).resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            false,
            true,
        );
        assert!(!resolved_with_cache.no_cache);
    }

    // ── Override resolution edge cases ───────────────────────────────

    #[test]
    fn resolve_override_with_invalid_glob_skipped() {
        let mut config = make_config(false);
        config.overrides = vec![ConfigOverride {
            files: vec!["[invalid".to_string()],
            rules: PartialRulesConfig {
                unused_files: Some(Severity::Off),
                ..Default::default()
            },
        }];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        // Invalid glob should be skipped, so no overrides should be compiled
        assert!(
            resolved.overrides.is_empty(),
            "override with invalid glob should be skipped"
        );
    }

    #[test]
    fn resolve_override_with_empty_files_skipped() {
        let mut config = make_config(false);
        config.overrides = vec![ConfigOverride {
            files: vec![],
            rules: PartialRulesConfig {
                unused_files: Some(Severity::Off),
                ..Default::default()
            },
        }];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(
            resolved.overrides.is_empty(),
            "override with no file patterns should be skipped"
        );
    }

    #[test]
    fn resolve_multiple_valid_overrides() {
        let mut config = make_config(false);
        config.overrides = vec![
            ConfigOverride {
                files: vec!["*.test.ts".to_string()],
                rules: PartialRulesConfig {
                    unused_exports: Some(Severity::Off),
                    ..Default::default()
                },
            },
            ConfigOverride {
                files: vec!["*.stories.tsx".to_string()],
                rules: PartialRulesConfig {
                    unused_files: Some(Severity::Off),
                    ..Default::default()
                },
            },
        ];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert_eq!(resolved.overrides.len(), 2);
    }

    // ── IgnoreExportRule ────────────────────────────────────────────

    #[test]
    fn ignore_export_rule_deserialize() {
        let json = r#"{"file": "src/types/*.ts", "exports": ["*"]}"#;
        let rule: IgnoreExportRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.file, "src/types/*.ts");
        assert_eq!(rule.exports, vec!["*"]);
    }

    #[test]
    fn ignore_export_rule_specific_exports() {
        let json = r#"{"file": "src/constants.ts", "exports": ["FOO", "BAR", "BAZ"]}"#;
        let rule: IgnoreExportRule = serde_json::from_str(json).unwrap();
        assert_eq!(rule.exports.len(), 3);
        assert!(rule.exports.contains(&"FOO".to_string()));
    }

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_resolved_config(production: bool) -> ResolvedConfig {
            make_config(production).resolve(
                PathBuf::from("/project"),
                OutputFormat::Human,
                1,
                true,
                true,
            )
        }

        proptest! {
            /// Resolved config always has non-empty ignore patterns (defaults are always added).
            #[test]
            fn resolved_config_has_default_ignores(production in any::<bool>()) {
                let resolved = arb_resolved_config(production);
                // Default patterns include node_modules, dist, build, .git, coverage, *.min.js, *.min.mjs
                prop_assert!(
                    resolved.ignore_patterns.is_match("node_modules/foo/bar.js"),
                    "Default ignore should match node_modules"
                );
                prop_assert!(
                    resolved.ignore_patterns.is_match("dist/bundle.js"),
                    "Default ignore should match dist"
                );
            }

            /// Production mode always forces dev and optional deps to Off.
            #[test]
            fn production_forces_dev_deps_off(_unused in Just(())) {
                let resolved = arb_resolved_config(true);
                prop_assert_eq!(
                    resolved.rules.unused_dev_dependencies,
                    Severity::Off,
                    "Production should force unused_dev_dependencies off"
                );
                prop_assert_eq!(
                    resolved.rules.unused_optional_dependencies,
                    Severity::Off,
                    "Production should force unused_optional_dependencies off"
                );
            }

            /// Non-production mode preserves default severity for dev deps.
            #[test]
            fn non_production_preserves_dev_deps_default(_unused in Just(())) {
                let resolved = arb_resolved_config(false);
                prop_assert_eq!(
                    resolved.rules.unused_dev_dependencies,
                    Severity::Warn,
                    "Non-production should keep default dev dep severity"
                );
            }

            /// Cache dir is always root/.fallow.
            #[test]
            fn cache_dir_is_root_fallow(dir_suffix in "[a-zA-Z0-9_]{1,20}") {
                let root = PathBuf::from(format!("/project/{dir_suffix}"));
                let expected_cache = root.join(".fallow");
                let resolved = make_config(false).resolve(
                    root,
                    OutputFormat::Human,
                    1,
                    true,
                    true,
                );
                prop_assert_eq!(
                    resolved.cache_dir, expected_cache,
                    "Cache dir should be root/.fallow"
                );
            }

            /// Thread count is always passed through exactly.
            #[test]
            fn threads_passed_through(threads in 1..64usize) {
                let resolved = make_config(false).resolve(
                    PathBuf::from("/project"),
                    OutputFormat::Human,
                    threads,
                    true,
                    true,
                );
                prop_assert_eq!(
                    resolved.threads, threads,
                    "Thread count should be passed through"
                );
            }

            /// Custom ignore patterns are merged with defaults, not replacing them.
            /// Uses a pattern regex that cannot match node_modules paths, so the
            /// assertion proves the default pattern is what provides the match.
            #[test]
            fn custom_ignores_dont_replace_defaults(pattern in "[a-z_]{1,10}/[a-z_]{1,10}") {
                let mut config = make_config(false);
                config.ignore_patterns = vec![pattern];
                let resolved = config.resolve(
                    PathBuf::from("/project"),
                    OutputFormat::Human,
                    1,
                    true,
                    true,
                );
                // Defaults should still be present (the custom pattern cannot
                // match this path, so only the default **/node_modules/** can)
                prop_assert!(
                    resolved.ignore_patterns.is_match("node_modules/foo/bar.js"),
                    "Default node_modules ignore should still be active"
                );
            }
        }
    }

    // ── Boundary preset expansion ──────────────────────────────────

    #[test]
    fn resolve_expands_boundary_preset() {
        use crate::config::boundaries::BoundaryPreset;

        let mut config = make_config(false);
        config.boundaries.preset = Some(BoundaryPreset::Hexagonal);
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        // Preset should have been expanded into zones (no tsconfig → fallback to "src")
        assert_eq!(resolved.boundaries.zones.len(), 3);
        assert_eq!(resolved.boundaries.rules.len(), 3);
        assert_eq!(resolved.boundaries.zones[0].name, "adapters");
        assert_eq!(
            resolved.boundaries.classify_zone("src/adapters/http.ts"),
            Some("adapters")
        );
    }

    #[test]
    fn resolve_boundary_preset_with_user_override() {
        use crate::config::boundaries::{BoundaryPreset, BoundaryZone};

        let mut config = make_config(false);
        config.boundaries.preset = Some(BoundaryPreset::Hexagonal);
        config.boundaries.zones = vec![BoundaryZone {
            name: "domain".to_string(),
            patterns: vec!["src/core/**".to_string()],
            root: None,
        }];
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        // User zone "domain" replaced preset zone "domain"
        assert_eq!(resolved.boundaries.zones.len(), 3);
        // The user's pattern should be used for domain zone
        assert_eq!(
            resolved.boundaries.classify_zone("src/core/user.ts"),
            Some("domain")
        );
        // Original preset pattern should NOT match
        assert_eq!(
            resolved.boundaries.classify_zone("src/domain/user.ts"),
            None
        );
    }

    #[test]
    fn resolve_no_preset_unchanged() {
        let config = make_config(false);
        let resolved = config.resolve(
            PathBuf::from("/project"),
            OutputFormat::Human,
            1,
            true,
            true,
        );
        assert!(resolved.boundaries.is_empty());
    }
}
