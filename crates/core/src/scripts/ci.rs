//! CI config file scanner for dependency usage detection.
//!
//! Extracts shell commands from `.gitlab-ci.yml` and `.github/workflows/*.yml`
//! files, parses them for binary invocations (especially `npx`), and maps
//! binaries to npm package names. This prevents false "unused dependency"
//! reports for packages only used in CI pipelines.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use super::{
    IgnoredCommandEntries, ScriptCatalog, analyze_commands_with_context, could_be_file_path,
};

/// Result of scanning CI config files: package names used by CI tooling AND
/// project-relative file paths referenced as command-line arguments.
#[derive(Debug, Default)]
pub struct CiAnalysis {
    /// npm package names used as binaries in CI shell commands.
    pub used_packages: FxHashSet<String>,
    /// File paths extracted as positional arguments or `--config` values
    /// (e.g., `node scripts/deploy.ts` in a GitHub Actions `run:` block).
    /// Paths are project-root-relative; CI files always live at the root.
    pub entry_files: Vec<String>,
}

/// Inputs shared by every CI file of one analysis.
struct CiContext<'a> {
    root: &'a Path,
    bin_map: &'a FxHashMap<String, String>,
    declared_packages: &'a FxHashSet<String>,
    scripts: &'a ScriptCatalog,
    ignored: IgnoredCommandEntries<'a>,
}

/// Analyze CI config files for package binary invocations and file references.
///
/// Scans GitLab CI and GitHub Actions workflow files for shell commands,
/// extracts binary names AND positional file path arguments, and returns both
/// the set of npm package names used and the file paths referenced as command
/// arguments. The file paths are seeded as entry points so scripts invoked from
/// CI (`node scripts/deploy.ts`) do not get reported as `unused-files`.
///
/// CI files always live at `.gitlab-ci.yml` or `.github/workflows/*.yml`
/// relative to the project root, so no workspace-prefix transformation applies.
/// The catalog spans the whole project, so a CI step that resolves to a script
/// declared only by a workspace package credits that body's dependencies but
/// contributes none of its file arguments, which are relative to that package.
pub fn analyze_ci_files(
    root: &Path,
    bin_map: &FxHashMap<String, String>,
    declared_packages: &FxHashSet<String>,
    scripts: &ScriptCatalog,
    ignored: IgnoredCommandEntries<'_>,
) -> CiAnalysis {
    let _span = tracing::info_span!("analyze_ci_files").entered();
    let mut analysis = CiAnalysis::default();
    let context = CiContext {
        root,
        bin_map,
        declared_packages,
        scripts,
        ignored,
    };

    let gitlab_ci = root.join(".gitlab-ci.yml");
    if let Ok(content) = std::fs::read_to_string(&gitlab_ci) {
        extract_ci_signals(&content, &context, &mut analysis);
    }

    let workflows_dir = root.join(".github/workflows");
    if let Ok(entries) = std::fs::read_dir(&workflows_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if (name_str.ends_with(".yml") || name_str.ends_with(".yaml"))
                && let Ok(content) = std::fs::read_to_string(entry.path())
            {
                extract_ci_signals(&content, &context, &mut analysis);
            }
        }
    }

    analysis.entry_files.sort();
    analysis.entry_files.dedup();
    analysis
}

/// Extract package names AND file path references from shell commands found in
/// a CI config file.
///
/// Uses line-based heuristics to find shell command lines in YAML CI configs.
/// This intentionally avoids a full YAML parser to keep dependencies minimal.
/// Known limitations (line-based parsing): variable interpolation
/// (`${{ matrix.env }}/deploy.ts`), `\` line-continuations, YAML anchors
/// (`<<: *defaults`) are silently skipped.
fn extract_ci_signals(content: &str, context: &CiContext<'_>, analysis: &mut CiAnalysis) {
    let commands = extract_ci_commands(content);
    let parsed = analyze_commands_with_context(
        &commands,
        context.root,
        context.bin_map,
        context.declared_packages,
        context.scripts,
        context.ignored,
    );
    analysis.used_packages.extend(parsed.used_packages);
    analysis.entry_files.extend(
        parsed
            .config_files
            .into_iter()
            .filter(|s| could_be_file_path(s)),
    );
    analysis.entry_files.extend(parsed.entry_files);
}

/// Extract shell command strings from a CI config file.
///
/// Recognizes:
/// - YAML list items in script blocks: `  - npx tool --flag`
/// - GitHub Actions run fields: `  run: command`
/// - Block scalar run blocks: `  run: |` or `  run: >` followed by indented lines
/// - Plain multi-line scalars: `  run: command` whose continuation lines are
///   indented past the `run` key column and fold into the same command
fn extract_ci_commands(content: &str) -> Vec<String> {
    let mut commands = Vec::new();
    let mut multiline_run = MultilineRunState::default();

    for line in content.lines() {
        let trimmed = line.trim();

        if should_skip_ci_line(trimmed) {
            continue;
        }

        if push_multiline_run_command(line, trimmed, &mut multiline_run, &mut commands) {
            continue;
        }

        if let Some(rest) = yaml_run_value(trimmed) {
            if is_multiline_run_marker(rest) {
                multiline_run.start(run_key_column(line, trimmed), false);
            } else if !rest.is_empty() {
                commands.push(rest.to_string());
                multiline_run.start(run_key_column(line, trimmed), true);
            }
            continue;
        }

        push_yaml_list_command(trimmed, &mut commands);
    }

    commands
}

#[derive(Default)]
struct MultilineRunState {
    active: bool,
    indent: usize,
    folding: bool,
}

impl MultilineRunState {
    /// Anchor at the `run` key column so sibling step keys (`env:`, `with:`),
    /// which sit at the same column, terminate the scalar instead of being
    /// swallowed as continuation lines. `folding` marks a plain scalar whose
    /// continuations fold into the already-pushed command.
    fn start(&mut self, key_column: usize, folding: bool) {
        self.active = true;
        self.indent = key_column;
        self.folding = folding;
    }

    fn stop(&mut self) {
        self.active = false;
        self.folding = false;
    }
}

/// Column of the `run` key itself, past any leading `- ` list indicator.
fn run_key_column(line: &str, trimmed: &str) -> usize {
    let base = line.len() - line.trim_start().len();
    match trimmed.strip_prefix("- ") {
        Some(rest) => base + 2 + (rest.len() - rest.trim_start().len()),
        None => base,
    }
}

fn should_skip_ci_line(trimmed: &str) -> bool {
    trimmed.is_empty() || trimmed.starts_with('#')
}

fn push_multiline_run_command(
    line: &str,
    trimmed: &str,
    state: &mut MultilineRunState,
    commands: &mut Vec<String>,
) -> bool {
    if !state.active {
        return false;
    }

    let indent = line.len() - line.trim_start().len();
    if indent > state.indent && !trimmed.is_empty() {
        if state.folding {
            if let Some(last) = commands.last_mut() {
                last.push(' ');
                last.push_str(trimmed);
            }
        } else {
            commands.push(trimmed.to_string());
        }
        return true;
    }

    state.stop();
    false
}

fn yaml_run_value(trimmed: &str) -> Option<&str> {
    strip_yaml_key(trimmed, "run")
        .or_else(|| {
            trimmed
                .strip_prefix("- ")
                .and_then(|rest| strip_yaml_key(rest.trim(), "run"))
        })
        .map(str::trim)
}

/// Recognize a YAML block scalar header on a `run:` value.
///
/// Accepts the literal (`|`) and folded (`>`) styles with any combination of a
/// chomping indicator (`-`, `+`) and an explicit indentation indicator (`1`-`9`),
/// in either order, matching the YAML block header grammar. A header carrying a
/// trailing comment is still not recognized.
fn is_multiline_run_marker(value: &str) -> bool {
    let mut chars = value.chars();
    if !matches!(chars.next(), Some('|' | '>')) {
        return false;
    }

    let mut chomping = false;
    let mut indentation = false;
    for ch in chars {
        match ch {
            '-' | '+' if !chomping => chomping = true,
            '1'..='9' if !indentation => indentation = true,
            _ => return false,
        }
    }
    true
}

fn push_yaml_list_command(trimmed: &str, commands: &mut Vec<String>) {
    let Some(rest) = trimmed.strip_prefix("- ") else {
        return;
    };
    let rest = rest.trim();
    if !rest.is_empty()
        && !rest.starts_with('{')
        && !rest.starts_with('[')
        && !is_yaml_mapping(rest)
    {
        commands.push(rest.to_string());
    }
}

/// Strip a YAML key prefix from a line, returning the value part.
/// Handles `key: value` and `key:` (empty value).
fn strip_yaml_key<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(key)?;
    let rest = rest.strip_prefix(':')?;
    Some(rest)
}

/// Check if a string looks like a YAML mapping (key: value) rather than a shell command.
fn is_yaml_mapping(s: &str) -> bool {
    if let Some(first_word) = s.split_whitespace().next()
        && first_word.ends_with(':')
        && !first_word.starts_with("http")
        && !first_word.starts_with("ftp")
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_set() -> FxHashSet<String> {
        FxHashSet::default()
    }

    fn set(values: &[&str]) -> FxHashSet<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn catalog(scripts: &[(&str, &str)]) -> ScriptCatalog {
        #[expect(
            clippy::disallowed_types,
            reason = "ScriptCatalog is built from serde-deserialized HashMap"
        )]
        let scripts: std::collections::HashMap<String, String> = scripts
            .iter()
            .map(|(name, body)| ((*name).to_string(), (*body).to_string()))
            .collect();
        ScriptCatalog::from_scripts(&scripts)
    }

    fn analyze_content(content: &str) -> CiAnalysis {
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &empty_set(),
                scripts: &catalog(&[]),
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        analysis
    }

    #[test]
    fn gitlab_ci_script_items() {
        let content = r"
stages:
  - build
  - test

build:
  stage: build
  script:
    - npm ci
    - npx @cyclonedx/cyclonedx-npm --output-file sbom.json
    - npm run build
";
        let commands = extract_ci_commands(content);
        assert!(commands.contains(&"npm ci".to_string()));
        assert!(
            commands.contains(&"npx @cyclonedx/cyclonedx-npm --output-file sbom.json".to_string())
        );
        assert!(commands.contains(&"npm run build".to_string()));
    }

    #[test]
    fn github_actions_inline_run() {
        let content = r"
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: npm ci
      - run: npx eslint src
";
        let commands = extract_ci_commands(content);
        assert!(commands.contains(&"npm ci".to_string()));
        assert!(commands.contains(&"npx eslint src".to_string()));
    }

    #[test]
    fn github_actions_multiline_run() {
        let content = r"
jobs:
  build:
    steps:
      - run: |
          npm ci
          npx @cyclonedx/cyclonedx-npm --output sbom.json
          npm run build
";
        let commands = extract_ci_commands(content);
        assert!(commands.contains(&"npm ci".to_string()));
        assert!(commands.contains(&"npx @cyclonedx/cyclonedx-npm --output sbom.json".to_string()));
        assert!(commands.contains(&"npm run build".to_string()));
    }

    /// The shape of GitHub's own code-scanning/eslint.yml starter workflow: a
    /// plain (unquoted) scalar whose continuation lines are indented past the
    /// `run` key column and fold into one command (issue #2016).
    #[test]
    fn github_actions_plain_multiline_run_folds_continuations() {
        let content = r"
jobs:
  eslint:
    steps:
      - name: Run ESLint
        run: npx eslint .
          --config .eslintrc.js
          --ext .js,.jsx,.ts,.tsx
        continue-on-error: true
";
        let commands = extract_ci_commands(content);
        assert!(
            commands.contains(
                &"npx eslint . --config .eslintrc.js --ext .js,.jsx,.ts,.tsx".to_string()
            ),
            "continuation lines must fold into the run command, got: {commands:?}"
        );
        assert!(
            !commands.iter().any(|c| c.contains("continue-on-error")),
            "sibling step keys must not be swallowed as continuations, got: {commands:?}"
        );
    }

    /// Sibling keys sit at the `run` key column, so key-column anchoring must
    /// terminate a plain scalar there; their path-looking values must not flow
    /// into entry_files where they could hide genuinely unused files.
    #[test]
    fn plain_run_sibling_key_values_do_not_leak_into_entry_files() {
        let content = r"
jobs:
  build:
    steps:
      - run: npx eslint .
          --max-warnings 0
        env:
          CONFIG_PATH: scripts/config.ts
";
        let commands = extract_ci_commands(content);
        assert!(commands.contains(&"npx eslint . --max-warnings 0".to_string()));
        assert!(!commands.iter().any(|c| c.contains("CONFIG_PATH")));

        let analysis = analyze_content(content);
        assert!(
            !analysis.entry_files.iter().any(|f| f.contains("config.ts")),
            "env values must not seed entry files, got: {:?}",
            analysis.entry_files
        );
    }

    /// End-to-end coverage for folded block scalars, beyond the marker
    /// predicate test: commands inside `run: >-` must reach package analysis.
    #[test]
    fn folded_run_block_commands_analyzed() {
        let content = r"
jobs:
  sbom:
    steps:
      - run: >-
          npx @cyclonedx/cyclonedx-npm
          --output-file sbom.json
";
        let analysis = analyze_content(content);
        assert!(analysis.used_packages.contains("@cyclonedx/cyclonedx-npm"));
    }

    #[test]
    fn yaml_mappings_filtered() {
        let content = r"
image: node:18
stages:
  - build
variables:
  NODE_ENV: production
build:
  script:
    - npm ci
";
        let commands = extract_ci_commands(content);
        assert!(!commands.iter().any(|c| c.contains("node:18")));
        assert!(!commands.iter().any(|c| c.contains("NODE_ENV")));
        assert!(commands.contains(&"npm ci".to_string()));
    }

    #[test]
    fn comments_and_empty_lines_skipped() {
        let content = r"
# This is a comment
  # Indented comment

build:
  script:
    - npm ci
";
        let commands = extract_ci_commands(content);
        assert_eq!(commands, vec!["npm ci"]);
    }

    #[test]
    fn npx_package_extracted() {
        let content = r"
build:
  script:
    - npx @cyclonedx/cyclonedx-npm --output-file sbom.json
";
        let analysis = analyze_content(content);
        let packages = &analysis.used_packages;
        assert!(
            packages.contains("@cyclonedx/cyclonedx-npm"),
            "packages: {packages:?}"
        );
    }

    #[test]
    fn multiple_binaries_extracted() {
        let content = r"
build:
  script:
    - npx eslint src
    - npx prettier --check .
    - tsc --noEmit
";
        let analysis = analyze_content(content);
        let packages = &analysis.used_packages;
        assert!(packages.contains("eslint"));
        assert!(packages.contains("prettier"));
        assert!(packages.contains("typescript")); // tsc → typescript via resolve
    }

    #[test]
    fn builtin_commands_not_extracted() {
        let content = r"
build:
  script:
    - echo 'hello'
    - mkdir -p dist
    - cp -r build/* dist/
";
        let analysis = analyze_content(content);
        let packages = &analysis.used_packages;
        assert!(
            packages.is_empty(),
            "should not extract built-in commands: {packages:?}"
        );
    }

    #[test]
    fn github_actions_npx_extracted() {
        let content = r"
jobs:
  sbom:
    steps:
      - run: npx @cyclonedx/cyclonedx-npm --output-file sbom.json
";
        let analysis = analyze_content(content);
        let packages = &analysis.used_packages;
        assert!(packages.contains("@cyclonedx/cyclonedx-npm"));
    }

    #[test]
    fn github_actions_pnpm_bare_declared_binary_extracted() {
        let content = r"
jobs:
  info:
    steps:
      - run: pnpm envinfo --system
";
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &set(&["envinfo"]),
                scripts: &catalog(&[]),
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        assert!(analysis.used_packages.contains("envinfo"));
    }

    #[test]
    fn github_actions_pnpm_script_name_shorthand_skipped() {
        let content = r"
jobs:
  build:
    steps:
      - run: pnpm build
";
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &set(&["build"]),
                scripts: &catalog(&[("build", "vite build")]),
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        assert!(!analysis.used_packages.contains("build"));
    }

    /// The dominant real-world shape: CI never names the linter, it runs the
    /// package.json script and adds the CI formatter flag (issue #2016).
    #[test]
    fn github_actions_npm_run_script_forwards_formatter_flag() {
        let content = r"
jobs:
  lint:
    steps:
      - run: npm run lint -- --format gha
";
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &set(&["eslint", "eslint-formatter-gha"]),
                scripts: &catalog(&[("lint", "eslint .")]),
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        assert!(analysis.used_packages.contains("eslint"));
        assert!(analysis.used_packages.contains("eslint-formatter-gha"));
    }

    /// A root CI step resolving to a workspace-only script must not turn that
    /// body's file arguments into root-relative entry patterns (issue #2016).
    #[test]
    fn workspace_only_script_body_does_not_seed_root_entry_files() {
        let content = r"
jobs:
  build:
    steps:
      - run: npm run build -- --mode ci
";
        #[expect(
            clippy::disallowed_types,
            reason = "ScriptCatalog is built from serde-deserialized HashMap"
        )]
        let ws_scripts: std::collections::HashMap<String, String> =
            std::iter::once(("build".to_string(), "esbuild scripts/bundle.js".to_string()))
                .collect();
        let mut scripts = ScriptCatalog::default();
        scripts.merge_workspace_scripts(&ws_scripts);

        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &set(&["esbuild"]),
                scripts: &scripts,
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        assert!(analysis.used_packages.contains("esbuild"));
        assert!(analysis.entry_files.is_empty());
    }

    /// Bun runs a declared script first. When no script has the name and the
    /// argument is a script file, Bun runs that file.
    #[test]
    fn github_actions_bun_file_runner_seeds_entry_files() {
        let content = r"
jobs:
  build:
    steps:
      - run: bun scripts/a.ts
      - run: bun run scripts/b.ts
      - run: bun --watch scripts/c.ts
      - run: bun run build --minify
      - run: bun run scripts/d.ts
      - run: bun run dev
";
        let mut analysis = CiAnalysis::default();
        extract_ci_signals(
            content,
            &CiContext {
                root: Path::new("/nonexistent"),
                bin_map: &FxHashMap::default(),
                declared_packages: &set(&["esbuild"]),
                scripts: &catalog(&[("build", "esbuild --bundle"), ("scripts/d.ts", "tsc")]),
                ignored: IgnoredCommandEntries::NONE,
            },
            &mut analysis,
        );
        assert_eq!(
            analysis.entry_files,
            vec!["scripts/a.ts", "scripts/b.ts", "scripts/c.ts"]
        );
        assert!(analysis.used_packages.contains("esbuild"));
        assert!(!analysis.used_packages.contains("bun"));
    }

    #[test]
    fn github_actions_expression_fragments_not_entry_files() {
        let content = r#"
jobs:
  health-check:
    steps:
      - run: |
          RESPONSE_CODE=$(curl -s -o "$TMPFILE" -w "%{http_code}" -m 15 "${{ env.ENVIRONMENT_URL }}/api/health/ready")
          echo "$RESPONSE_CODE"
"#;
        let analysis = analyze_content(content);
        for path in &analysis.entry_files {
            assert!(
                !path.contains("${{") && !path.contains("}}"),
                "entry_files must not contain GitHub Actions expression fragments, got: {path:?}"
            );
        }
    }

    #[test]
    fn jq_array_iterator_not_entry_file() {
        let content = r#"
jobs:
  process:
    steps:
      - run: |
          jq -c '.[]' /tmp/x.json | while read item; do echo "$item"; done
          result=$(jq -r '.[]' data.json)
"#;
        let analysis = analyze_content(content);
        for path in &analysis.entry_files {
            assert!(
                !path.contains(".[]"),
                "entry_files must not contain jq array-iterator fragments, got: {path:?}"
            );
        }
    }

    #[test]
    fn grep_perl_regex_fragment_not_entry_file() {
        let content = r"
jobs:
  deploy:
    steps:
      - run: |
          grep -oP '(?<=Module )\./[^ ]+(?= has finished with an error)' deploy.log
";
        let analysis = analyze_content(content);
        for path in &analysis.entry_files {
            assert!(
                !path.contains(r"\./"),
                "entry_files must not contain regex escape fragments, got: {path:?}"
            );
            assert!(
                !path.contains("[^"),
                "entry_files must not contain unclosed character class fragments, got: {path:?}"
            );
        }
    }

    /// Regression test for issue #2592: a quoted jq filter passed as a shell
    /// argument to `jq -r` (the alternative operator `//` gives it a `/`)
    /// must not be harvested as an entry pattern. Reported against the exact
    /// `mapfile < <(jq ...)` shape from a real workflow.
    #[test]
    fn jq_alternative_operator_filter_not_entry_file() {
        let content = r#"
jobs:
  process:
    steps:
      - name: List proposal PRs
        run: |
          state=".github/state.json"
          mapfile -t proposal_prs < <(jq -r '[((.proposals // {}) | to_entries[]) | .value.pr_number] | unique | sort[]' "$state")
"#;
        let analysis = analyze_content(content);
        assert!(
            analysis.entry_files.is_empty(),
            "jq filter must not become an entry pattern, got: {:?}",
            analysis.entry_files
        );
    }

    /// A `run:` block invoking a real script by path is genuine dependency
    /// evidence and must still be harvested, even next to a `run:` block
    /// whose quoted jq filter must be dropped (issue #2592).
    #[test]
    fn genuine_script_path_alongside_jq_filter_still_harvested() {
        let content = r#"
jobs:
  process:
    steps:
      - name: List proposal PRs
        run: |
          state=".github/state.json"
          mapfile -t proposal_prs < <(jq -r '[((.proposals // {}) | to_entries[]) | .value.pr_number] | unique | sort[]' "$state")
      - name: Run real script
        run: node scripts/deploy.js --env production
"#;
        let analysis = analyze_content(content);
        assert!(
            analysis
                .entry_files
                .contains(&"scripts/deploy.js".to_string()),
            "genuine script path must still be harvested, got: {:?}",
            analysis.entry_files
        );
        assert!(
            !analysis.entry_files.iter().any(|f| f.contains("proposals")),
            "jq filter must not leak into entry_files, got: {:?}",
            analysis.entry_files
        );
    }

    #[test]
    fn strip_yaml_key_basic() {
        assert_eq!(strip_yaml_key("run: npm test", "run"), Some(" npm test"));
        assert_eq!(strip_yaml_key("run:", "run"), Some(""));
        assert_eq!(strip_yaml_key("other: value", "run"), None);
    }

    #[test]
    fn is_yaml_mapping_basic() {
        assert!(is_yaml_mapping("NODE_ENV: production"));
        assert!(is_yaml_mapping("image: node:18"));
        assert!(!is_yaml_mapping("npm ci"));
        assert!(!is_yaml_mapping("npx eslint src"));
        assert!(!is_yaml_mapping("https://example.com"));
    }

    /// GitHub Actions accepts the folded style as readily as the literal one, and
    /// fallow's own release-validation workflow uses it.
    #[test]
    fn folded_run_scalar_is_a_block_marker() {
        for marker in ["|", "|-", "|+", ">", ">-", ">+", "|2", ">2-", ">-2"] {
            assert!(
                is_multiline_run_marker(marker),
                "{marker} is a block header"
            );
        }
        for value in [
            "", "npm ci", "|foo", ">out.txt", "||", ">>", "-", "|0", "|--",
        ] {
            assert!(
                !is_multiline_run_marker(value),
                "{value} is a command, not a block header"
            );
        }
    }
}
