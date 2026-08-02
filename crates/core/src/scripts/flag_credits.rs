//! CLI flag-value dependency crediting.
//!
//! Some CLIs load an npm package because a flag value names it: `eslint
//! --format gha` loads `eslint-formatter-gha`, `mocha --reporter mochawesome`
//! loads `mochawesome`. A package used only that way has no import, no config
//! entry, and no binary invocation anywhere in the project, so without a credit
//! it is reported as an unused dependency (issues #2006 and #2019).
//!
//! The conventions are data, not code: the `(binary, flag) -> resolution` rows
//! live in `crates/core/data/cli_flag_credits.toml`, embedded via
//! `include_str!` and parsed once at startup. There is no regeneration step.
//!
//! A credit only ever exempts an already-declared dependency from the unused
//! scan; it is never consulted by unlisted-dependency detection, so a
//! synthesized name cannot invent a finding.

use rustc_hash::FxHashMap;

use super::strip_surrounding_quotes;

/// Embedded catalogue source. Because it is `include_str!`-embedded at compile
/// time, a green `catalogue_parses` test guarantees the released binary parses.
const CATALOGUE_TOML: &str = include_str!("../../data/cli_flag_credits.toml");

/// File extensions that mark an unscoped verbatim value as a file path rather
/// than a package name.
const SCRIPT_EXTENSIONS: &[&str] = &[
    ".js", ".cjs", ".mjs", ".jsx", ".ts", ".cts", ".mts", ".tsx", ".json", ".node",
];

/// How a tool maps a flag value to the package it loads.
///
/// The set is closed on purpose: each variant encodes a documented resolution
/// algorithm, so an unknown value in the TOML is a bug rather than a new rule.
/// Serde rejects unknown variants, which makes the catalogue parse fail loudly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Resolution {
    /// The tool expands a bare value with the entry's prefix the way eslint's
    /// `normalizePackageName` does; scoped values are always packages, unscoped
    /// values with a path character are files.
    Prefixed,
    /// The tool tries the prefix-expanded name first and the plain name second
    /// (jest-resolve), so both candidates are credited.
    PrefixedThenBare,
    /// The value is the package name itself, optionally with a subpath
    /// (`dotenv/config` credits `dotenv`).
    Verbatim,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogueFile {
    #[serde(default, rename = "flag-credit")]
    flag_credit: Vec<FlagCreditEntry>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FlagCreditEntry {
    /// CLI names the rule applies to, after package-manager wrappers are
    /// stripped.
    binaries: Vec<String>,
    /// Flag spellings that take the package-naming value.
    flags: Vec<String>,
    resolution: Resolution,
    /// Required for the prefixed resolutions, forbidden for verbatim.
    #[serde(default)]
    prefix: Option<String>,
    /// Values the tool resolves internally; they credit nothing.
    #[serde(default)]
    builtins: Vec<String>,
    /// Optional human context; does not affect matching.
    #[expect(
        dead_code,
        reason = "documentation field, surfaced via the catalogue source"
    )]
    #[serde(default)]
    notes: Option<String>,
}

/// One parsed convention shared by every `(binary, flag)` key it covers.
#[derive(Debug)]
struct FlagRule {
    resolution: Resolution,
    prefix: Option<String>,
    builtins: Vec<String>,
}

#[derive(Debug)]
struct Catalogue {
    rules: Vec<FlagRule>,
    /// `(binary, flag)` -> index into `rules`.
    index: FxHashMap<(String, String), usize>,
}

impl Catalogue {
    fn rule(&self, binary: &str, flag: &str) -> Option<&FlagRule> {
        let key = (binary.to_string(), flag.to_string());
        self.index.get(&key).map(|&idx| &self.rules[idx])
    }
}

/// Parse and validate catalogue source.
///
/// # Errors
///
/// Returns a human-readable message when the TOML is malformed, an entry names
/// an unknown resolution, a binary, flag, or builtin is blank, a flag does not
/// start with `-`, the prefix contract for the resolution is violated, or two
/// entries claim the same `(binary, flag)` key.
fn parse(source: &str) -> Result<Catalogue, String> {
    let parsed: CatalogueFile = toml::from_str(source).map_err(|e| e.to_string())?;
    let mut rules = Vec::new();
    let mut index = FxHashMap::default();

    for entry in parsed.flag_credit {
        let label = format!("{:?} / {:?}", entry.binaries, entry.flags);
        if entry.binaries.is_empty() {
            return Err(format!("flag-credit entry {label} lists no binaries"));
        }
        if entry.flags.is_empty() {
            return Err(format!("flag-credit entry {label} lists no flags"));
        }
        if entry.binaries.iter().any(|b| b.trim().is_empty()) {
            return Err(format!("flag-credit entry {label} has a blank binary"));
        }
        if let Some(flag) = entry
            .flags
            .iter()
            .find(|f| !f.starts_with('-') || f.len() < 2)
        {
            return Err(format!(
                "flag-credit entry {label} has a malformed flag {flag:?}"
            ));
        }
        if entry.builtins.iter().any(|b| b.trim().is_empty()) {
            return Err(format!("flag-credit entry {label} has a blank builtin"));
        }
        match entry.resolution {
            Resolution::Prefixed | Resolution::PrefixedThenBare => {
                if entry.prefix.as_deref().is_none_or(|p| p.trim().is_empty()) {
                    return Err(format!(
                        "flag-credit entry {label} needs a non-empty prefix for its resolution"
                    ));
                }
            }
            Resolution::Verbatim => {
                if entry.prefix.is_some() {
                    return Err(format!(
                        "flag-credit entry {label} is verbatim and must not set a prefix"
                    ));
                }
            }
        }

        let rule_idx = rules.len();
        rules.push(FlagRule {
            resolution: entry.resolution,
            prefix: entry.prefix,
            builtins: entry.builtins,
        });
        for binary in &entry.binaries {
            for flag in &entry.flags {
                let key = (binary.clone(), flag.clone());
                if index.contains_key(&key) {
                    return Err(format!("duplicate flag-credit entry: {key:?}"));
                }
                index.insert(key, rule_idx);
            }
        }
    }

    Ok(Catalogue { rules, index })
}

/// Parse and cache the embedded catalogue once. Panics with a clear message if
/// the embedded TOML is invalid; this is unreachable in a released binary
/// because the bytes are compile-time-embedded and gated by `catalogue_parses`.
#[expect(
    clippy::expect_used,
    reason = "embedded flag-credit catalogue is compile-time data pinned by catalogue_parses"
)]
fn catalogue() -> &'static Catalogue {
    static CATALOGUE: std::sync::OnceLock<Catalogue> = std::sync::OnceLock::new();
    CATALOGUE.get_or_init(|| {
        parse(CATALOGUE_TOML).expect(
            "embedded crates/core/data/cli_flag_credits.toml must be valid; run \
             `cargo test -p fallow-core flag_credits` to see the error",
        )
    })
}

/// Packages a CLI names through a flag value rather than a positional argument.
///
/// `args` are the tokens after the binary itself; scanning stops at `--`
/// because everything past it belongs to another program.
pub fn flag_referenced_packages(binary: &str, args: &[&str]) -> Vec<String> {
    let catalogue = catalogue();
    let mut packages = Vec::new();
    let mut idx = 0;
    while idx < args.len() {
        let token = args[idx];
        if token == "--" {
            break;
        }

        let inline = token
            .split_once('=')
            .and_then(|(flag, value)| catalogue.rule(binary, flag).map(|rule| (rule, value)));
        let (rule, value) = if let Some(found) = inline {
            idx += 1;
            found
        } else if let Some(rule) = catalogue.rule(binary, token) {
            idx += 2;
            let Some(value) = args.get(idx - 1) else {
                break;
            };
            (rule, *value)
        } else {
            idx += 1;
            continue;
        };

        for piece in strip_surrounding_quotes(value).split(',') {
            credit_value(rule, piece.trim(), &mut packages);
        }
    }
    packages
}

fn credit_value(rule: &FlagRule, value: &str, out: &mut Vec<String>) {
    // A leading dash is a flag, not a value; a colon marks node: built-ins,
    // Windows drive paths, and URLs, none of which are npm packages.
    if value.is_empty() || value.starts_with('-') || value.contains(':') {
        return;
    }
    if rule.builtins.iter().any(|builtin| builtin == value) {
        return;
    }

    let prefix = rule.prefix.as_deref();
    match rule.resolution {
        Resolution::Prefixed => {
            if let Some(prefix) = prefix
                && let Some(package) = prefixed_package(value, prefix)
            {
                push_unique(out, package);
            }
        }
        Resolution::PrefixedThenBare => {
            if value.starts_with('@') {
                // jest-resolve's prefix expansion is not scope-aware; a scoped
                // value only ever resolves to the scoped package itself.
                if let Some(package) = verbatim_package(value) {
                    push_unique(out, package);
                }
                return;
            }
            if let Some(prefix) = prefix
                && let Some(package) = prefixed_package(value, prefix)
            {
                push_unique(out, package);
            }
            if let Some(package) = verbatim_package(value) {
                push_unique(out, package);
            }
        }
        Resolution::Verbatim => {
            if let Some(package) = verbatim_package(value) {
                push_unique(out, package);
            }
        }
    }
}

fn push_unique(out: &mut Vec<String>, package: String) {
    if !out.contains(&package) {
        out.push(package);
    }
}

/// Resolve a prefix-expanded flag value to the package the tool would load.
///
/// Mirrors eslint's `normalizePackageName(name, prefix)`: a value starting with
/// `@` is always a package, never a path, so `@scope/sarif` with prefix
/// `eslint-formatter` becomes `@scope/eslint-formatter-sarif` and an
/// already-qualified name passes through. Only an unscoped value carrying a
/// path separator or extension is a file path.
fn prefixed_package(value: &str, prefix: &str) -> Option<String> {
    if let Some(scoped) = value.strip_prefix('@') {
        let mut parts = scoped.splitn(2, '/');
        let scope = parts.next().filter(|s| !s.is_empty())?;
        return Some(match parts.next().filter(|s| !s.is_empty()) {
            None => format!("@{scope}/{prefix}"),
            Some(name) if name.starts_with(prefix) => format!("@{scope}/{name}"),
            Some(name) => format!("@{scope}/{prefix}-{name}"),
        });
    }

    if value.contains(['/', '\\', '.']) {
        return None;
    }
    if let Some(rest) = value.strip_prefix(prefix)
        && rest.starts_with('-')
    {
        return Some(value.to_string());
    }
    Some(format!("{prefix}-{value}"))
}

/// Resolve a verbatim flag value to the package that owns the specifier.
///
/// A subpath specifier credits the owning package: `dotenv/config` credits
/// `dotenv`, `@scope/pkg/register` credits `@scope/pkg`. Relative and absolute
/// paths credit nothing, and an unscoped first segment ending in a script
/// extension is a file, not a package.
fn verbatim_package(value: &str) -> Option<String> {
    if value.starts_with(['.', '/', '\\']) {
        return None;
    }
    if let Some(scoped) = value.strip_prefix('@') {
        let mut parts = scoped.split('/');
        let scope = parts.next().filter(|s| !s.is_empty())?;
        let name = parts.next().filter(|s| !s.is_empty())?;
        return Some(format!("@{scope}/{name}"));
    }
    let first = value.split('/').next()?;
    if first.is_empty()
        || first.contains('\\')
        || SCRIPT_EXTENSIONS.iter().any(|ext| first.ends_with(ext))
    {
        return None;
    }
    Some(first.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credits(binary: &str, command: &str) -> Vec<String> {
        let tokens: Vec<&str> = command.split_whitespace().collect();
        flag_referenced_packages(binary, &tokens)
    }

    fn formatter_packages(command: &str) -> Vec<String> {
        credits("eslint", command)
    }

    #[test]
    fn catalogue_parses() {
        let cat = catalogue();
        assert!(!cat.index.is_empty(), "catalogue must have entries");
    }

    #[test]
    fn bare_formatter_shorthand_expands() {
        assert_eq!(
            formatter_packages("--format gha"),
            vec!["eslint-formatter-gha"]
        );
        assert_eq!(formatter_packages("-f gha"), vec!["eslint-formatter-gha"]);
        assert_eq!(
            formatter_packages("--format=gha"),
            vec!["eslint-formatter-gha"]
        );
    }

    /// GitHub's own code-scanning starter workflow passes exactly this value.
    #[test]
    fn scoped_formatter_is_credited_verbatim() {
        assert_eq!(
            formatter_packages("--format @microsoft/eslint-formatter-sarif"),
            vec!["@microsoft/eslint-formatter-sarif"]
        );
    }

    #[test]
    fn scoped_shorthand_formatter_expands() {
        assert_eq!(
            formatter_packages("--format @microsoft/sarif"),
            vec!["@microsoft/eslint-formatter-sarif"]
        );
        assert_eq!(
            formatter_packages("--format @scope"),
            vec!["@scope/eslint-formatter"]
        );
    }

    #[test]
    fn already_prefixed_formatter_is_not_double_prefixed() {
        assert_eq!(
            formatter_packages("--format eslint-formatter-gha"),
            vec!["eslint-formatter-gha"]
        );
    }

    #[test]
    fn unscoped_path_formatter_is_skipped() {
        assert!(formatter_packages("--format ./tools/fmt.js").is_empty());
        assert!(formatter_packages("--format node_modules/x/index.js").is_empty());
    }

    #[test]
    fn tokens_after_double_dash_are_not_scanned() {
        assert!(formatter_packages("--fix -- --format gha").is_empty());
    }

    #[test]
    fn flag_value_starting_with_dash_credits_nothing() {
        assert!(formatter_packages("--format --fix").is_empty());
    }

    #[test]
    fn eslint_plugin_expands_with_plugin_prefix() {
        assert_eq!(
            credits("eslint", "--plugin import"),
            vec!["eslint-plugin-import"]
        );
        assert_eq!(
            credits("eslint", "--plugin import,unicorn"),
            vec!["eslint-plugin-import", "eslint-plugin-unicorn"]
        );
    }

    #[test]
    fn jest_environment_credits_both_candidates() {
        assert_eq!(
            credits("jest", "--testEnvironment jsdom"),
            vec!["jest-environment-jsdom", "jsdom"]
        );
        assert_eq!(
            credits("jest", "--env=jsdom"),
            vec!["jest-environment-jsdom", "jsdom"]
        );
    }

    #[test]
    fn jest_builtin_environment_credits_nothing() {
        assert!(credits("jest", "--testEnvironment node").is_empty());
    }

    #[test]
    fn jest_already_prefixed_environment_is_credited_once() {
        assert_eq!(
            credits("jest", "--testEnvironment jest-environment-jsdom"),
            vec!["jest-environment-jsdom"]
        );
    }

    #[test]
    fn jest_scoped_environment_is_credited_verbatim() {
        assert_eq!(
            credits("jest", "--testEnvironment @happy-dom/jest-environment"),
            vec!["@happy-dom/jest-environment"]
        );
    }

    #[test]
    fn jest_reporter_is_verbatim_and_builtins_abstain() {
        assert_eq!(
            credits("jest", "--reporters jest-junit"),
            vec!["jest-junit"]
        );
        assert!(credits("jest", "--reporters default").is_empty());
        assert!(credits("jest", "--reporters github-actions").is_empty());
    }

    #[test]
    fn node_preload_subpath_credits_owning_package() {
        assert_eq!(credits("node", "-r dotenv/config"), vec!["dotenv"]);
        assert_eq!(
            credits("node", "--require ts-node/register"),
            vec!["ts-node"]
        );
        assert_eq!(credits("node", "--import tsx src/main.ts"), vec!["tsx"]);
    }

    #[test]
    fn node_builtin_and_path_preloads_credit_nothing() {
        assert!(credits("node", "-r node:assert").is_empty());
        assert!(credits("node", "-r ./setup.js").is_empty());
        assert!(credits("node", "-r setup.js").is_empty());
        assert!(credits("node", "--loader /abs/loader.mjs").is_empty());
    }

    #[test]
    fn mocha_reporter_is_verbatim_and_builtins_abstain() {
        assert_eq!(
            credits("mocha", "--reporter mochawesome"),
            vec!["mochawesome"]
        );
        assert!(credits("mocha", "--reporter spec").is_empty());
        assert!(
            credits("mocha", "-R json").is_empty(),
            "built-in colliding with a real npm package must abstain"
        );
    }

    #[test]
    fn prettier_stylelint_postcss_values_are_verbatim() {
        assert_eq!(
            credits("prettier", "--plugin prettier-plugin-tailwindcss"),
            vec!["prettier-plugin-tailwindcss"]
        );
        assert_eq!(
            credits("stylelint", "--custom-syntax postcss-scss"),
            vec!["postcss-scss"]
        );
        assert_eq!(
            credits("postcss", "--use autoprefixer"),
            vec!["autoprefixer"]
        );
    }

    #[test]
    fn unlisted_binary_or_flag_credits_nothing() {
        assert!(credits("prettier", "--format gha").is_empty());
        assert!(credits("stylelint", "--formatter pretty").is_empty());
        assert!(credits("nyc", "--reporter lcov").is_empty());
    }

    #[test]
    fn unknown_resolution_is_rejected() {
        let err = parse(
            r#"
[[flag-credit]]
binaries = ["x"]
flags = ["--y"]
resolution = "fuzzy"
"#,
        )
        .expect_err("unknown resolution must fail");
        assert!(err.contains("fuzzy"), "got: {err}");
    }

    #[test]
    fn unknown_field_is_rejected() {
        let err = parse(
            r#"
[[flag-credit]]
binaries = ["x"]
flags = ["--y"]
resolution = "verbatim"
ecosystem = "css"
"#,
        )
        .expect_err("unknown field must fail");
        assert!(err.contains("ecosystem"), "got: {err}");
    }

    #[test]
    fn prefixed_without_prefix_is_rejected() {
        parse(
            r#"
[[flag-credit]]
binaries = ["x"]
flags = ["--y"]
resolution = "prefixed"
"#,
        )
        .expect_err("prefixed resolution without prefix must fail");
    }

    #[test]
    fn verbatim_with_prefix_is_rejected() {
        parse(
            r#"
[[flag-credit]]
binaries = ["x"]
flags = ["--y"]
resolution = "verbatim"
prefix = "x-plugin"
"#,
        )
        .expect_err("verbatim resolution with prefix must fail");
    }

    #[test]
    fn malformed_flag_is_rejected() {
        parse(
            r#"
[[flag-credit]]
binaries = ["x"]
flags = ["plugin"]
resolution = "verbatim"
"#,
        )
        .expect_err("flag without leading dash must fail");
    }

    #[test]
    fn duplicate_binary_flag_pair_is_rejected() {
        let err = parse(
            r#"
[[flag-credit]]
binaries = ["x"]
flags = ["--y"]
resolution = "verbatim"

[[flag-credit]]
binaries = ["x"]
flags = ["--y", "-z"]
resolution = "verbatim"
"#,
        )
        .expect_err("duplicate (binary, flag) must fail");
        assert!(err.contains("duplicate"), "got: {err}");
    }

    #[test]
    fn empty_binaries_or_flags_are_rejected() {
        parse(
            r#"
[[flag-credit]]
binaries = []
flags = ["--y"]
resolution = "verbatim"
"#,
        )
        .expect_err("empty binaries must fail");
        parse(
            r#"
[[flag-credit]]
binaries = ["x"]
flags = []
resolution = "verbatim"
"#,
        )
        .expect_err("empty flags must fail");
    }
}
