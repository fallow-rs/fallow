//! Metric and rule definitions for explainable CLI output.
//!
//! Provides structured metadata that describes what each metric, threshold,
//! and rule means — consumed by the `_meta` object in JSON output and by
//! SARIF `fullDescription` / `helpUri` fields.

use std::process::ExitCode;

use colored::Colorize;
use fallow_config::OutputFormat;
use serde_json::{Value, json};

// ── Docs base URL ────────────────────────────────────────────────

const DOCS_BASE: &str = "https://docs.fallow.tools";

/// Docs URL for the dead-code (check) command.
pub const CHECK_DOCS: &str = "https://docs.fallow.tools/cli/dead-code";

/// Docs URL for the health command.
pub const HEALTH_DOCS: &str = "https://docs.fallow.tools/cli/health";

/// Docs URL for the dupes command.
pub const DUPES_DOCS: &str = "https://docs.fallow.tools/cli/dupes";

/// Docs URL for the runtime coverage setup command's agent-readable JSON.
pub const COVERAGE_SETUP_DOCS: &str = "https://docs.fallow.tools/cli/coverage#agent-readable-json";

/// Docs URL for `fallow coverage analyze --format json --explain`.
pub const COVERAGE_ANALYZE_DOCS: &str = "https://docs.fallow.tools/cli/coverage#analyze";

// ── Check rules ─────────────────────────────────────────────────

/// Rule definition for SARIF `fullDescription` and JSON `_meta`.
pub struct RuleDef {
    pub id: &'static str,
    /// Coarse category label used by the sticky PR/MR comment renderer to
    /// group findings into collapsible sections (Dead code, Dependencies,
    /// Duplication, Health, Architecture, Suppressions). One source of
    /// truth so the CodeClimate / SARIF / review-envelope path and the
    /// renderer never drift; a unit test below asserts every RuleDef has
    /// a non-empty category.
    pub category: &'static str,
    pub name: &'static str,
    pub short: &'static str,
    pub full: &'static str,
    pub docs_path: &'static str,
}

pub const CHECK_RULES: &[RuleDef] = &[
    RuleDef {
        id: "fallow/unused-file",
        category: "Dead code",
        name: "Unused Files",
        short: "File is not reachable from any entry point",
        full: "Source files that are not imported by any other module and are not entry points (scripts, tests, configs). These files can safely be deleted. Detection uses graph reachability from configured entry points.",
        docs_path: "explanations/dead-code#unused-files",
    },
    RuleDef {
        id: "fallow/unused-export",
        category: "Dead code",
        name: "Unused Exports",
        short: "Export is never imported",
        full: "Named exports that are never imported by any other module in the project. Includes both direct exports and re-exports through barrel files. The export may still be used locally within the same file.",
        docs_path: "explanations/dead-code#unused-exports",
    },
    RuleDef {
        id: "fallow/unused-type",
        category: "Dead code",
        name: "Unused Type Exports",
        short: "Type export is never imported",
        full: "Type-only exports (interfaces, type aliases, enums used only as types) that are never imported. These do not generate runtime code but add maintenance burden.",
        docs_path: "explanations/dead-code#unused-types",
    },
    RuleDef {
        id: "fallow/private-type-leak",
        category: "Dead code",
        name: "Private Type Leaks",
        short: "Exported signature references a private type",
        full: "Exported values or types whose public TypeScript signature references a same-file type declaration that is not exported. Consumers cannot name that private type directly, so the backing type should be exported or removed from the public signature.",
        docs_path: "explanations/dead-code#private-type-leaks",
    },
    RuleDef {
        id: "fallow/unused-dependency",
        category: "Dependencies",
        name: "Unused Dependencies",
        short: "Dependency listed but never imported",
        full: "Packages listed in dependencies that are never imported or required by any source file. Framework plugins and CLI tools may be false positives; use the ignore_dependencies config to suppress.",
        docs_path: "explanations/dead-code#unused-dependencies",
    },
    RuleDef {
        id: "fallow/unused-dev-dependency",
        category: "Dependencies",
        name: "Unused Dev Dependencies",
        short: "Dev dependency listed but never imported",
        full: "Packages listed in devDependencies that are never imported by test files, config files, or scripts. Build tools and jest presets that are referenced only in config may appear as false positives.",
        docs_path: "explanations/dead-code#unused-devdependencies",
    },
    RuleDef {
        id: "fallow/unused-optional-dependency",
        category: "Dependencies",
        name: "Unused Optional Dependencies",
        short: "Optional dependency listed but never imported",
        full: "Packages listed in optionalDependencies that are never imported. Optional dependencies are typically platform-specific; verify they are not needed on any supported platform before removing.",
        docs_path: "explanations/dead-code#unused-optionaldependencies",
    },
    RuleDef {
        id: "fallow/type-only-dependency",
        category: "Dependencies",
        name: "Type-only Dependencies",
        short: "Production dependency only used via type-only imports",
        full: "Production dependencies that are only imported via `import type` statements. These can be moved to devDependencies since they generate no runtime code and are stripped during compilation.",
        docs_path: "explanations/dead-code#type-only-dependencies",
    },
    RuleDef {
        id: "fallow/test-only-dependency",
        category: "Dependencies",
        name: "Test-only Dependencies",
        short: "Production dependency only imported by test files",
        full: "Production dependencies that are only imported from test files. These can usually move to devDependencies because production entry points do not require them at runtime.",
        docs_path: "explanations/dead-code#test-only-dependencies",
    },
    RuleDef {
        id: "fallow/unused-enum-member",
        category: "Dead code",
        name: "Unused Enum Members",
        short: "Enum member is never referenced",
        full: "Enum members that are never referenced in the codebase. Uses scope-aware binding analysis to track all references including computed access patterns.",
        docs_path: "explanations/dead-code#unused-enum-members",
    },
    RuleDef {
        id: "fallow/unused-class-member",
        category: "Dead code",
        name: "Unused Class Members",
        short: "Class member is never referenced",
        full: "Class methods and properties that are never referenced outside the class. Private members are checked within the class scope; public members are checked project-wide.",
        docs_path: "explanations/dead-code#unused-class-members",
    },
    RuleDef {
        id: "fallow/unresolved-import",
        category: "Dead code",
        name: "Unresolved Imports",
        short: "Import could not be resolved",
        full: "Import specifiers that could not be resolved to a file on disk. Common causes: deleted files, typos in paths, missing path aliases in tsconfig, or uninstalled packages.",
        docs_path: "explanations/dead-code#unresolved-imports",
    },
    RuleDef {
        id: "fallow/unlisted-dependency",
        category: "Dependencies",
        name: "Unlisted Dependencies",
        short: "Dependency used but not in package.json",
        full: "Packages that are imported in source code but not listed in package.json. These work by accident (hoisted from another workspace package or transitive dep) and will break in strict package managers.",
        docs_path: "explanations/dead-code#unlisted-dependencies",
    },
    RuleDef {
        id: "fallow/duplicate-export",
        category: "Dead code",
        name: "Duplicate Exports",
        short: "Export name appears in multiple modules",
        full: "The same export name is defined in multiple modules. Consumers may import from the wrong module, leading to subtle bugs. Consider renaming or consolidating.",
        docs_path: "explanations/dead-code#duplicate-exports",
    },
    RuleDef {
        id: "fallow/circular-dependency",
        category: "Architecture",
        name: "Circular Dependencies",
        short: "Circular dependency chain detected",
        full: "A cycle in the module import graph. Circular dependencies cause undefined behavior with CommonJS (partial modules) and initialization ordering issues with ESM. Break cycles by extracting shared code.",
        docs_path: "explanations/dead-code#circular-dependencies",
    },
    RuleDef {
        id: "fallow/boundary-violation",
        category: "Architecture",
        name: "Boundary Violations",
        short: "Import crosses a configured architecture boundary",
        full: "A module imports from a zone that its configured boundary rules do not allow. Boundary checks help keep layered architecture, feature slices, and package ownership rules enforceable.",
        docs_path: "explanations/dead-code#boundary-violations",
    },
    RuleDef {
        id: "fallow/stale-suppression",
        category: "Suppressions",
        name: "Stale Suppressions",
        short: "Suppression comment or tag no longer matches any issue",
        full: "A fallow-ignore-next-line, fallow-ignore-file, or @expected-unused suppression that no longer matches any active issue. The underlying problem was fixed but the suppression was left behind. Remove it to keep the codebase clean.",
        docs_path: "explanations/dead-code#stale-suppressions",
    },
];

/// Look up a rule definition by its SARIF rule ID across all rule sets.
#[must_use]
pub fn rule_by_id(id: &str) -> Option<&'static RuleDef> {
    CHECK_RULES
        .iter()
        .chain(HEALTH_RULES.iter())
        .chain(DUPES_RULES.iter())
        .find(|r| r.id == id)
}

/// Build the docs URL for a rule.
#[must_use]
pub fn rule_docs_url(rule: &RuleDef) -> String {
    format!("{DOCS_BASE}/{}", rule.docs_path)
}

/// Extra educational content for the standalone `fallow explain <issue-type>`
/// command. Kept separate from [`RuleDef`] so SARIF and `_meta` payloads remain
/// compact while terminal users and agents can ask for worked examples on
/// demand.
pub struct RuleGuide {
    pub example: &'static str,
    pub how_to_fix: &'static str,
}

/// Look up an issue type from a user-facing token.
///
/// Accepts canonical SARIF ids (`fallow/unused-export`), issue tokens
/// (`unused-export`), and common CLI filter spellings (`unused-exports`).
#[must_use]
pub fn rule_by_token(token: &str) -> Option<&'static RuleDef> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rule) = rule_by_id(trimmed) {
        return Some(rule);
    }
    let normalized = trimmed
        .strip_prefix("fallow/")
        .unwrap_or(trimmed)
        .trim_start_matches("--")
        .replace('_', "-");
    let alias = match normalized.as_str() {
        "unused-files" => Some("fallow/unused-file"),
        "unused-exports" => Some("fallow/unused-export"),
        "unused-types" => Some("fallow/unused-type"),
        "private-type-leaks" => Some("fallow/private-type-leak"),
        "unused-deps" | "unused-dependencies" => Some("fallow/unused-dependency"),
        "unused-dev-deps" | "unused-dev-dependencies" => Some("fallow/unused-dev-dependency"),
        "unused-optional-deps" | "unused-optional-dependencies" => {
            Some("fallow/unused-optional-dependency")
        }
        "type-only-deps" | "type-only-dependencies" => Some("fallow/type-only-dependency"),
        "test-only-deps" | "test-only-dependencies" => Some("fallow/test-only-dependency"),
        "unused-enum-members" => Some("fallow/unused-enum-member"),
        "unused-class-members" => Some("fallow/unused-class-member"),
        "unresolved-imports" => Some("fallow/unresolved-import"),
        "unlisted-deps" | "unlisted-dependencies" => Some("fallow/unlisted-dependency"),
        "duplicate-exports" => Some("fallow/duplicate-export"),
        "circular-deps" | "circular-dependencies" => Some("fallow/circular-dependency"),
        "boundary-violations" => Some("fallow/boundary-violation"),
        "stale-suppressions" => Some("fallow/stale-suppression"),
        "complexity" | "high-complexity" => Some("fallow/high-complexity"),
        "cyclomatic" | "high-cyclomatic" | "high-cyclomatic-complexity" => {
            Some("fallow/high-cyclomatic-complexity")
        }
        "cognitive" | "high-cognitive" | "high-cognitive-complexity" => {
            Some("fallow/high-cognitive-complexity")
        }
        "crap" | "high-crap" | "high-crap-score" => Some("fallow/high-crap-score"),
        "duplication" | "dupes" | "code-duplication" => Some("fallow/code-duplication"),
        _ => None,
    };
    if let Some(id) = alias
        && let Some(rule) = rule_by_id(id)
    {
        return Some(rule);
    }
    let singular = normalized
        .strip_suffix('s')
        .filter(|_| normalized != "unused-class")
        .unwrap_or(&normalized);
    let id = format!("fallow/{singular}");
    rule_by_id(&id).or_else(|| {
        CHECK_RULES
            .iter()
            .chain(HEALTH_RULES.iter())
            .chain(DUPES_RULES.iter())
            .find(|rule| {
                rule.docs_path.ends_with(&normalized)
                    || rule.docs_path.ends_with(singular)
                    || rule.name.eq_ignore_ascii_case(trimmed)
            })
    })
}

/// Return worked-example and fix guidance for a rule.
#[must_use]
pub fn rule_guide(rule: &RuleDef) -> RuleGuide {
    match rule.id {
        "fallow/unused-file" => RuleGuide {
            example: "src/old-widget.ts is not imported by any entry point, route, script, or config file.",
            how_to_fix: "Delete the file if it is genuinely dead. If a framework loads it implicitly, add the right plugin/config pattern or mark it in alwaysUsed.",
        },
        "fallow/unused-export" => RuleGuide {
            example: "export const formatPrice = ... exists in src/money.ts, but no module imports formatPrice.",
            how_to_fix: "Remove the export or make it file-local. If it is public API, import it from an entry point or add an intentional suppression with context.",
        },
        "fallow/unused-type" => RuleGuide {
            example: "export interface LegacyProps is exported, but no module imports the type.",
            how_to_fix: "Remove the type export, inline it, or keep it behind an explicit API entry point when consumers rely on it.",
        },
        "fallow/private-type-leak" => RuleGuide {
            example: "export function makeUser(): InternalUser exposes InternalUser even though InternalUser is not exported.",
            how_to_fix: "Export the referenced type, change the public signature to an exported type, or keep the helper private.",
        },
        "fallow/unused-dependency"
        | "fallow/unused-dev-dependency"
        | "fallow/unused-optional-dependency" => RuleGuide {
            example: "package.json lists left-pad, but no source, script, config, or plugin-recognized file imports it.",
            how_to_fix: "Remove the dependency after checking runtime/plugin usage. If another workspace uses it, move the dependency to that workspace.",
        },
        "fallow/type-only-dependency" => RuleGuide {
            example: "zod is in dependencies but only appears in import type declarations.",
            how_to_fix: "Move the package to devDependencies unless runtime code imports it as a value.",
        },
        "fallow/test-only-dependency" => RuleGuide {
            example: "vitest is listed in dependencies, but only test files import it.",
            how_to_fix: "Move the package to devDependencies unless production code imports it at runtime.",
        },
        "fallow/unused-enum-member" => RuleGuide {
            example: "Status.Legacy remains in an exported enum, but no code reads that member.",
            how_to_fix: "Remove the member after checking serialized/API compatibility, or suppress it with a reason when external data still uses it.",
        },
        "fallow/unused-class-member" => RuleGuide {
            example: "class Parser has a public parseLegacy method that is never called in the project.",
            how_to_fix: "Remove or privatize the member. For reflection/framework lifecycle hooks, configure or suppress the intentional entry point.",
        },
        "fallow/unresolved-import" => RuleGuide {
            example: "src/app.ts imports ./routes/admin, but no matching file exists after extension and index resolution.",
            how_to_fix: "Fix the specifier, restore the missing file, install the package, or align tsconfig path aliases with the runtime resolver.",
        },
        "fallow/unlisted-dependency" => RuleGuide {
            example: "src/api.ts imports undici, but the nearest package.json does not list undici.",
            how_to_fix: "Add the package to dependencies/devDependencies in the workspace that imports it instead of relying on hoisting or transitive deps.",
        },
        "fallow/duplicate-export" => RuleGuide {
            example: "Button is exported from both src/ui/button.ts and src/components/button.ts.",
            how_to_fix: "Rename or consolidate the exports so consumers have one intentional import target.",
        },
        "fallow/circular-dependency" => RuleGuide {
            example: "src/a.ts imports src/b.ts, and src/b.ts imports src/a.ts.",
            how_to_fix: "Extract shared code to a third module, invert the dependency, or split initialization-time side effects from type-only contracts.",
        },
        "fallow/boundary-violation" => RuleGuide {
            example: "features/billing imports app/admin even though the configured boundary only allows imports from shared and entities.",
            how_to_fix: "Move the shared contract to an allowed zone, invert the dependency, or update the boundary config only if the architecture rule was wrong.",
        },
        "fallow/stale-suppression" => RuleGuide {
            example: "// fallow-ignore-next-line unused-export remains above an export that is now used.",
            how_to_fix: "Remove the suppression. If a different issue is still intentional, replace it with a current, specific suppression.",
        },
        "fallow/high-cyclomatic-complexity"
        | "fallow/high-cognitive-complexity"
        | "fallow/high-complexity" => RuleGuide {
            example: "A function contains several nested conditionals, loops, and early exits, exceeding the configured complexity threshold.",
            how_to_fix: "Extract named helpers, split independent branches, flatten guard clauses, and add tests around the behavior before refactoring.",
        },
        "fallow/high-crap-score" => RuleGuide {
            example: "A complex function has little or no matching Istanbul coverage, so its CRAP score crosses the configured gate.",
            how_to_fix: "Add focused tests for the risky branches first, then simplify the function if the score remains high.",
        },
        "fallow/refactoring-target" => RuleGuide {
            example: "A file combines high complexity density, churn, fan-in, and dead-code signals.",
            how_to_fix: "Start with the listed evidence: remove dead exports, extract complex functions, then reduce fan-out or cycles in small steps.",
        },
        "fallow/untested-file" | "fallow/untested-export" => RuleGuide {
            example: "Production-reachable code has no dependency path from discovered test entry points.",
            how_to_fix: "Add or wire a test that imports the runtime path, or update entry-point/test discovery if the existing test is invisible to fallow.",
        },
        "fallow/runtime-safe-to-delete"
        | "fallow/runtime-review-required"
        | "fallow/runtime-low-traffic"
        | "fallow/runtime-coverage-unavailable"
        | "fallow/runtime-coverage" => RuleGuide {
            example: "Runtime coverage shows a function was never called, barely called, or could not be matched during the capture window.",
            how_to_fix: "Treat high-confidence cold static-dead code as delete candidates. For advisory or unavailable coverage, inspect seasonality, workers, source maps, and capture quality first.",
        },
        "fallow/code-duplication" => RuleGuide {
            example: "Two files contain the same normalized token sequence across a multi-line block.",
            how_to_fix: "Extract the shared logic when the duplicated behavior should evolve together. Leave it duplicated when the similarity is accidental and likely to diverge.",
        },
        _ => RuleGuide {
            example: "Run the relevant command with --format json --quiet --explain to inspect this rule in context.",
            how_to_fix: "Use the issue action hints, source location, and docs URL to decide whether to remove, move, configure, or suppress the finding.",
        },
    }
}

/// Run the standalone explain subcommand.
#[must_use]
pub fn run_explain(issue_type: &str, output: OutputFormat) -> ExitCode {
    let Some(rule) = rule_by_token(issue_type) else {
        return crate::error::emit_error(
            &format!(
                "unknown issue type '{issue_type}'. Try values like unused-export, unused-dependency, high-complexity, or code-duplication"
            ),
            2,
            output,
        );
    };
    let guide = rule_guide(rule);
    match output {
        OutputFormat::Json => crate::report::emit_json(
            &json!({
                "id": rule.id,
                "name": rule.name,
                "summary": rule.short,
                "rationale": rule.full,
                "example": guide.example,
                "how_to_fix": guide.how_to_fix,
                "docs": rule_docs_url(rule),
            }),
            "explain",
        ),
        OutputFormat::Human => print_explain_human(rule, &guide),
        OutputFormat::Compact => print_explain_compact(rule),
        OutputFormat::Markdown => print_explain_markdown(rule, &guide),
        OutputFormat::Sarif
        | OutputFormat::CodeClimate
        | OutputFormat::PrCommentGithub
        | OutputFormat::PrCommentGitlab
        | OutputFormat::ReviewGithub
        | OutputFormat::ReviewGitlab
        | OutputFormat::Badge => crate::error::emit_error(
            "explain supports human, compact, markdown, and json output",
            2,
            output,
        ),
    }
}

fn print_explain_human(rule: &RuleDef, guide: &RuleGuide) -> ExitCode {
    println!("{}", rule.name.bold());
    println!("{}", rule.id.dimmed());
    println!();
    println!("{}", rule.short);
    println!();
    println!("{}", "Why it matters".bold());
    println!("{}", rule.full);
    println!();
    println!("{}", "Example".bold());
    println!("{}", guide.example);
    println!();
    println!("{}", "How to fix".bold());
    println!("{}", guide.how_to_fix);
    println!();
    println!("{} {}", "Docs:".dimmed(), rule_docs_url(rule).dimmed());
    ExitCode::SUCCESS
}

fn print_explain_compact(rule: &RuleDef) -> ExitCode {
    println!("explain:{}:{}:{}", rule.id, rule.short, rule_docs_url(rule));
    ExitCode::SUCCESS
}

fn print_explain_markdown(rule: &RuleDef, guide: &RuleGuide) -> ExitCode {
    println!("# {}", rule.name);
    println!();
    println!("`{}`", rule.id);
    println!();
    println!("{}", rule.short);
    println!();
    println!("## Why it matters");
    println!();
    println!("{}", rule.full);
    println!();
    println!("## Example");
    println!();
    println!("{}", guide.example);
    println!();
    println!("## How to fix");
    println!();
    println!("{}", guide.how_to_fix);
    println!();
    println!("[Docs]({})", rule_docs_url(rule));
    ExitCode::SUCCESS
}

// ── Health SARIF rules ──────────────────────────────────────────

pub const HEALTH_RULES: &[RuleDef] = &[
    RuleDef {
        id: "fallow/high-cyclomatic-complexity",
        category: "Health",
        name: "High Cyclomatic Complexity",
        short: "Function has high cyclomatic complexity",
        full: "McCabe cyclomatic complexity exceeds the configured threshold. Cyclomatic complexity counts the number of independent paths through a function (1 + decision points: if/else, switch cases, loops, ternary, logical operators). High values indicate functions that are hard to test exhaustively.",
        docs_path: "explanations/health#cyclomatic-complexity",
    },
    RuleDef {
        id: "fallow/high-cognitive-complexity",
        category: "Health",
        name: "High Cognitive Complexity",
        short: "Function has high cognitive complexity",
        full: "SonarSource cognitive complexity exceeds the configured threshold. Unlike cyclomatic complexity, cognitive complexity penalizes nesting depth and non-linear control flow (breaks, continues, early returns). It measures how hard a function is to understand when reading sequentially.",
        docs_path: "explanations/health#cognitive-complexity",
    },
    RuleDef {
        id: "fallow/high-complexity",
        category: "Health",
        name: "High Complexity (Both)",
        short: "Function exceeds both complexity thresholds",
        full: "Function exceeds both cyclomatic and cognitive complexity thresholds. This is the strongest signal that a function needs refactoring, it has many paths AND is hard to understand.",
        docs_path: "explanations/health#complexity-metrics",
    },
    RuleDef {
        id: "fallow/high-crap-score",
        category: "Health",
        name: "High CRAP Score",
        short: "Function has a high CRAP score (complexity combined with low coverage)",
        full: "The function's CRAP (Change Risk Anti-Patterns) score meets or exceeds the configured threshold. CRAP combines cyclomatic complexity with test coverage using the Savoia and Evans (2007) formula: `CC^2 * (1 - coverage/100)^3 + CC`. High CRAP indicates changes to this function carry high risk because it is complex AND poorly tested. Pair with `--coverage` for accurate per-function scoring; without it fallow estimates coverage from the module graph.",
        docs_path: "explanations/health#crap-score",
    },
    RuleDef {
        id: "fallow/refactoring-target",
        category: "Health",
        name: "Refactoring Target",
        short: "File identified as a high-priority refactoring candidate",
        full: "File identified as a refactoring candidate based on a weighted combination of complexity density, churn velocity, dead code ratio, fan-in (blast radius), and fan-out (coupling). Categories: urgent churn+complexity, break circular dependency, split high-impact file, remove dead code, extract complex functions, reduce coupling.",
        docs_path: "explanations/health#refactoring-targets",
    },
    RuleDef {
        id: "fallow/untested-file",
        category: "Health",
        name: "Untested File",
        short: "Runtime-reachable file has no test dependency path",
        full: "A file is reachable from runtime entry points but not from any discovered test entry point. This indicates production code that no test imports, directly or transitively, according to the static module graph.",
        docs_path: "explanations/health#coverage-gaps",
    },
    RuleDef {
        id: "fallow/untested-export",
        category: "Health",
        name: "Untested Export",
        short: "Runtime-reachable export has no test dependency path",
        full: "A value export is reachable from runtime entry points but no test-reachable module references it. This is a static test dependency gap rather than line coverage, and highlights exports exercised only through production entry paths.",
        docs_path: "explanations/health#coverage-gaps",
    },
    RuleDef {
        id: "fallow/runtime-safe-to-delete",
        category: "Health",
        name: "Production Safe To Delete",
        short: "Statically unused AND never invoked in production with V8 tracking",
        full: "The function is both statically unreachable in the module graph and was never invoked during the observed runtime coverage window. This is the highest-confidence delete signal fallow emits.",
        docs_path: "explanations/health#runtime-coverage",
    },
    RuleDef {
        id: "fallow/runtime-review-required",
        category: "Health",
        name: "Production Review Required",
        short: "Statically used but never invoked in production",
        full: "The function is reachable in the module graph (or exercised by tests / untracked call sites) but was not invoked during the observed runtime coverage window. Needs a human look: may be seasonal, error-path only, or legitimately unused.",
        docs_path: "explanations/health#runtime-coverage",
    },
    RuleDef {
        id: "fallow/runtime-low-traffic",
        category: "Health",
        name: "Production Low Traffic",
        short: "Function was invoked below the low-traffic threshold",
        full: "The function was invoked in production but below the configured `--low-traffic-threshold` fraction of total trace count (spec default 0.1%). Effectively dead for the current period.",
        docs_path: "explanations/health#runtime-coverage",
    },
    RuleDef {
        id: "fallow/runtime-coverage-unavailable",
        category: "Health",
        name: "Runtime Coverage Unavailable",
        short: "Runtime coverage could not be resolved for this function",
        full: "The function could not be matched to a V8-tracked coverage entry. Common causes: the function lives in a worker thread (separate V8 isolate), it is lazy-parsed and never reached the JIT tier, or its source map did not resolve to the expected source path. This is advisory, not a dead-code signal.",
        docs_path: "explanations/health#runtime-coverage",
    },
    RuleDef {
        id: "fallow/runtime-coverage",
        category: "Health",
        name: "Runtime Coverage",
        short: "Runtime coverage finding",
        full: "Generic runtime-coverage finding for verdicts not covered by a more specific rule. Covers the forward-compat `unknown` sentinel; the CLI filters `active` entries out of `runtime_coverage.findings` so the surfaced list stays actionable.",
        docs_path: "explanations/health#runtime-coverage",
    },
];

pub const DUPES_RULES: &[RuleDef] = &[RuleDef {
    id: "fallow/code-duplication",
    category: "Duplication",
    name: "Code Duplication",
    short: "Duplicated code block",
    full: "A block of code that appears in multiple locations with identical or near-identical token sequences. Clone detection uses normalized token comparison: identifier names and literals are abstracted away in non-strict modes.",
    docs_path: "explanations/duplication#clone-groups",
}];

// ── JSON _meta builders ─────────────────────────────────────────

/// Build the `_meta` object for `fallow dead-code --format json --explain`.
#[must_use]
pub fn check_meta() -> Value {
    let rules: Value = CHECK_RULES
        .iter()
        .map(|r| {
            (
                r.id.replace("fallow/", ""),
                json!({
                    "name": r.name,
                    "description": r.full,
                    "docs": rule_docs_url(r)
                }),
            )
        })
        .collect::<serde_json::Map<String, Value>>()
        .into();

    json!({
        "docs": CHECK_DOCS,
        "rules": rules
    })
}

/// Build the `_meta` object for `fallow health --format json --explain`.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "flat metric table: every entry is 3-4 short lines of metadata and keeping them in one map is clearer than splitting into per-metric helpers"
)]
pub fn health_meta() -> Value {
    json!({
        "docs": HEALTH_DOCS,
        "metrics": {
            "cyclomatic": {
                "name": "Cyclomatic Complexity",
                "description": "McCabe cyclomatic complexity: 1 + number of decision points (if/else, switch cases, loops, ternary, logical operators). Measures the number of independent paths through a function.",
                "range": "[1, \u{221e})",
                "interpretation": "lower is better; default threshold: 20"
            },
            "cognitive": {
                "name": "Cognitive Complexity",
                "description": "SonarSource cognitive complexity: penalizes nesting depth and non-linear control flow (breaks, continues, early returns). Measures how hard a function is to understand when reading top-to-bottom.",
                "range": "[0, \u{221e})",
                "interpretation": "lower is better; default threshold: 15"
            },
            "line_count": {
                "name": "Function Line Count",
                "description": "Number of lines in the function body.",
                "range": "[1, \u{221e})",
                "interpretation": "context-dependent; long functions may need splitting"
            },
            "lines": {
                "name": "File Line Count",
                "description": "Total lines of code in the file (from line offsets). Provides scale context for other metrics: a file with 0.4 complexity density at 80 LOC is different from 0.4 density at 800 LOC.",
                "range": "[1, \u{221e})",
                "interpretation": "context-dependent; large files may benefit from splitting even if individual functions are small"
            },
            "maintainability_index": {
                "name": "Maintainability Index",
                "description": "Composite score: 100 - (complexity_density \u{00d7} 30 \u{00d7} dampening) - (dead_code_ratio \u{00d7} 20) - min(ln(fan_out+1) \u{00d7} 4, 15), where dampening = min(lines/50, 1.0). Clamped to [0, 100]. Higher is better.",
                "range": "[0, 100]",
                "interpretation": "higher is better; <40 poor, 40\u{2013}70 moderate, >70 good"
            },
            "complexity_density": {
                "name": "Complexity Density",
                "description": "Total cyclomatic complexity divided by lines of code. Measures how densely complex the code is per line.",
                "range": "[0, \u{221e})",
                "interpretation": "lower is better; >1.0 indicates very dense complexity"
            },
            "dead_code_ratio": {
                "name": "Dead Code Ratio",
                "description": "Fraction of value exports (excluding type-only exports like interfaces and type aliases) with zero references across the project.",
                "range": "[0, 1]",
                "interpretation": "lower is better; 0 = all exports are used"
            },
            "fan_in": {
                "name": "Fan-in (Importers)",
                "description": "Number of files that import this file. High fan-in means high blast radius \u{2014} changes to this file affect many dependents.",
                "range": "[0, \u{221e})",
                "interpretation": "context-dependent; high fan-in files need careful review before changes"
            },
            "fan_out": {
                "name": "Fan-out (Imports)",
                "description": "Number of files this file directly imports. High fan-out indicates high coupling and change propagation risk.",
                "range": "[0, \u{221e})",
                "interpretation": "lower is better; MI penalty caps at ~40 imports"
            },
            "score": {
                "name": "Hotspot Score",
                "description": "normalized_churn \u{00d7} normalized_complexity \u{00d7} 100, where normalization is against the project maximum. Identifies files that are both complex AND frequently changing.",
                "range": "[0, 100]",
                "interpretation": "higher = riskier; prioritize refactoring high-score files"
            },
            "weighted_commits": {
                "name": "Weighted Commits",
                "description": "Recency-weighted commit count using exponential decay with 90-day half-life. Recent commits contribute more than older ones.",
                "range": "[0, \u{221e})",
                "interpretation": "higher = more recent churn activity"
            },
            "trend": {
                "name": "Churn Trend",
                "description": "Compares recent vs older commit frequency within the analysis window. accelerating = recent > 1.5\u{00d7} older, cooling = recent < 0.67\u{00d7} older, stable = in between.",
                "values": ["accelerating", "stable", "cooling"],
                "interpretation": "accelerating files need attention; cooling files are stabilizing"
            },
            "priority": {
                "name": "Refactoring Priority",
                "description": "Weighted score: complexity density (30%), hotspot boost (25%), dead code ratio (20%), fan-in (15%), fan-out (10%). Fan-in and fan-out normalization uses adaptive percentile-based thresholds (p95 of the project distribution). Does not use the maintainability index to avoid double-counting.",
                "range": "[0, 100]",
                "interpretation": "higher = more urgent to refactor"
            },
            "efficiency": {
                "name": "Efficiency Score",
                "description": "priority / effort_numeric (Low=1, Medium=2, High=3). Surfaces quick wins: high-priority, low-effort targets rank first. Default sort order.",
                "range": "[0, 100] \u{2014} effective max depends on effort: Low=100, Medium=50, High\u{2248}33",
                "interpretation": "higher = better quick-win value; targets are sorted by efficiency descending"
            },
            "effort": {
                "name": "Effort Estimate",
                "description": "Heuristic effort estimate based on file size, function count, and fan-in. Thresholds adapt to the project\u{2019}s distribution (percentile-based). Low: small file, few functions, low fan-in. High: large file, high fan-in, or many functions with high density. Medium: everything else.",
                "values": ["low", "medium", "high"],
                "interpretation": "low = quick win, high = needs planning and coordination"
            },
            "confidence": {
                "name": "Confidence Level",
                "description": "Reliability of the recommendation based on data source. High: deterministic graph/AST analysis (dead code, circular deps, complexity). Medium: heuristic thresholds (fan-in/fan-out coupling). Low: depends on git history quality (churn-based recommendations).",
                "values": ["high", "medium", "low"],
                "interpretation": "high = act on it, medium = verify context, low = treat as a signal, not a directive"
            },
            "health_score": {
                "name": "Health Score",
                "description": "Project-level aggregate score computed from vital signs: dead code, complexity, maintainability, hotspots, unused dependencies, and circular dependencies. Penalties subtracted from 100. Missing metrics (from pipelines that didn't run) don't penalize. Use --score to compute the score; add --hotspots, or --targets with --score, when the score should include the churn-backed hotspot penalty.",
                "range": "[0, 100]",
                "interpretation": "higher is better; A (85\u{2013}100), B (70\u{2013}84), C (55\u{2013}69), D (40\u{2013}54), F (0\u{2013}39)"
            },
            "crap_max": {
                "name": "Untested Complexity Risk (CRAP)",
                "description": "Change Risk Anti-Patterns score (Savoia & Evans, 2007). Formula: CC\u{00b2} \u{00d7} (1 - cov/100)\u{00b3} + CC. Default model (static_estimated): estimates per-function coverage from export references \u{2014} directly test-referenced exports get 85%, indirectly test-reachable functions get 40%, untested files get 0%. Provide --coverage <path> with Istanbul-format coverage-final.json (from Jest, Vitest, c8, nyc) for exact per-function CRAP scores.",
                "range": "[1, \u{221e})",
                "interpretation": "lower is better; >=30 is high-risk (CC >= 5 without test path)"
            },
            "bus_factor": {
                "name": "Bus Factor",
                "description": "Avelino truck factor: the minimum number of distinct contributors who together account for at least 50% of recency-weighted commits to this file in the analysis window. Bot authors are excluded.",
                "range": "[1, \u{221e})",
                "interpretation": "lower is higher knowledge-loss risk; 1 means a single contributor covers most of the recent history"
            },
            "contributor_count": {
                "name": "Contributor Count",
                "description": "Number of distinct authors who touched this file in the analysis window after bot-pattern filtering.",
                "range": "[0, \u{221e})",
                "interpretation": "higher generally indicates broader knowledge spread; pair with bus_factor for context"
            },
            "share": {
                "name": "Contributor Share",
                "description": "Recency-weighted share of total weighted commits attributed to a single contributor. Rounded to three decimals.",
                "range": "[0, 1]",
                "interpretation": "share close to 1.0 indicates dominance and pairs with low bus_factor"
            },
            "stale_days": {
                "name": "Stale Days",
                "description": "Days since this contributor last touched the file. Computed at analysis time.",
                "range": "[0, \u{221e})",
                "interpretation": "high stale_days on the top contributor often correlates with ownership drift"
            },
            "drift": {
                "name": "Ownership Drift",
                "description": "True when the file's original author (earliest first commit in the window) differs from the current top contributor, the file is at least 30 days old, and the original author's recency-weighted share is below 10%.",
                "values": [true, false],
                "interpretation": "true means the original author is no longer maintaining; route reviews to the current top contributor"
            },
            "unowned": {
                "name": "Unowned (Tristate)",
                "description": "true = a CODEOWNERS file exists but no rule matches this file; false = a rule matches; null = no CODEOWNERS file was discovered for the repository (cannot determine).",
                "values": [true, false, null],
                "interpretation": "true on a hotspot is a review-bottleneck risk; null means the signal is unavailable, not absent"
            },
            "runtime_coverage_verdict": {
                "name": "Runtime Coverage Verdict",
                "description": "Overall verdict across all runtime-coverage findings. `clean` = nothing cold; `cold-code-detected` = one or more tracked functions had zero invocations; `hot-path-touched` = a function modified in the current change set is on the hot path; `license-expired-grace` = analysis ran but the license is in its post-expiry grace window; `unknown` = verdict could not be computed (degenerate input).",
                "values": ["clean", "hot-path-touched", "cold-code-detected", "license-expired-grace", "unknown"],
                "interpretation": "`cold-code-detected` is the primary actionable signal; `hot-path-touched` elevates code-review attention for touched hot paths"
            },
            "runtime_coverage_state": {
                "name": "Runtime Coverage State",
                "description": "Per-function observation: `called` = V8 saw at least one invocation; `never-called` = V8 tracked the function but it never ran; `coverage-unavailable` = the function was not in the V8 tracking set (e.g., lazy-parsed, worker thread, dynamic code); `unknown` = forward-compat sentinel for newer sidecar states.",
                "values": ["called", "never-called", "coverage-unavailable", "unknown"],
                "interpretation": "`never-called` in combination with static `unused` is the highest-confidence delete signal"
            },
            "runtime_coverage_confidence": {
                "name": "Runtime Coverage Confidence",
                "description": "Confidence in a runtime-coverage finding. `high` = tracked by V8 with a statistically meaningful observation volume; `medium` = either low observation volume or indirect evidence; `low` = minimal data; `unknown` = insufficient information to classify.",
                "values": ["high", "medium", "low", "unknown"],
                "interpretation": "high = act on it; medium = verify context; low = treat as a signal only"
            },
            "production_invocations": {
                "name": "Production Invocations",
                "description": "Observed invocation count for the function over the collected coverage window. For `coverage-unavailable` findings this is `0` and semantically means `null` (not tracked). Absolute counts are not directly comparable across services without normalizing by trace_count.",
                "range": "[0, \u{221e})",
                "interpretation": "0 + tracked = cold path; 0 + untracked = unknown; high + never-called cannot occur by definition"
            },
            "percent_dead_in_production": {
                "name": "Percent Dead in Production",
                "description": "Fraction of tracked functions with zero observed invocations, multiplied by 100. Computed before any `--top` truncation so the summary total is stable regardless of display limits.",
                "range": "[0, 100]",
                "interpretation": "lower is better; values above ~10% on a long-running service indicate a large cleanup opportunity"
            }
        }
    })
}

/// Build the `_meta` object for `fallow dupes --format json --explain`.
#[must_use]
pub fn dupes_meta() -> Value {
    json!({
        "docs": DUPES_DOCS,
        "metrics": {
            "duplication_percentage": {
                "name": "Duplication Percentage",
                "description": "Fraction of total source tokens that appear in at least one clone group. Computed over the full analyzed file set.",
                "range": "[0, 100]",
                "interpretation": "lower is better"
            },
            "token_count": {
                "name": "Token Count",
                "description": "Number of normalized source tokens in the clone group. Tokens are language-aware (keywords, identifiers, operators, punctuation). Higher token count = larger duplicate.",
                "range": "[1, \u{221e})",
                "interpretation": "larger clones have higher refactoring value"
            },
            "line_count": {
                "name": "Line Count",
                "description": "Number of source lines spanned by the clone instance. Approximation of clone size for human readability.",
                "range": "[1, \u{221e})",
                "interpretation": "larger clones are more impactful to deduplicate"
            },
            "clone_groups": {
                "name": "Clone Groups",
                "description": "A set of code fragments with identical or near-identical normalized token sequences. Each group has 2+ instances across different locations.",
                "interpretation": "each group is a single refactoring opportunity"
            },
            "clone_families": {
                "name": "Clone Families",
                "description": "Groups of clone groups that share the same set of files. Indicates systematic duplication patterns (e.g., mirrored directory structures).",
                "interpretation": "families suggest extract-module refactoring opportunities"
            }
        }
    })
}

/// Build the `_meta` object for `fallow coverage setup --json --explain`.
#[must_use]
pub fn coverage_setup_meta() -> Value {
    json!({
        "docs_url": COVERAGE_SETUP_DOCS,
        "field_definitions": {
            "schema_version": "Coverage setup JSON contract version. Stays at \"1\" for additive opt-in fields such as _meta.",
            "framework_detected": "Primary detected runtime framework for compatibility with single-app consumers. In workspaces this mirrors the first emitted runtime member; unknown means no runtime member was detected.",
            "package_manager": "Detected package manager used for install and run commands, or null when no package manager signal was found.",
            "runtime_targets": "Union of runtime targets across emitted members.",
            "members[]": "Per-runtime-workspace setup recipes. Pure aggregator roots and build-only libraries are omitted.",
            "members[].name": "Workspace package name from package.json, or the root directory name when package.json has no name.",
            "members[].path": "Workspace path relative to the command root. The root package is represented as \".\".",
            "members[].framework_detected": "Runtime framework detected for that member.",
            "members[].package_manager": "Package manager detected for that member, or inherited from the workspace root when no member-specific signal exists.",
            "members[].runtime_targets": "Runtime targets produced by that member.",
            "members[].files_to_edit": "Files in that member that should receive runtime beacon setup code.",
            "members[].snippets": "Copy-paste setup snippets for that member, with paths relative to the command root.",
            "members[].dockerfile_snippet": "Environment snippet for file-system capture in that member's containerized Node runtime, or null when not applicable.",
            "members[].warnings": "Actionable setup caveats discovered for that member.",
            "config_written": "Always null for --json because JSON setup is side-effect-free and never writes configuration.",
            "files_to_edit": "Compatibility copy of the primary member's files, with workspace prefixes when the primary member is not the root.",
            "snippets": "Compatibility copy of the primary member's snippets, with workspace prefixes when the primary member is not the root.",
            "dockerfile_snippet": "Environment snippet for file-system capture in containerized Node runtimes, or null when not applicable.",
            "commands": "Package-manager commands needed to install the runtime beacon and sidecar packages.",
            "next_steps": "Ordered setup workflow after applying the emitted snippets.",
            "warnings": "Actionable setup caveats discovered while building the recipe."
        },
        "enums": {
            "framework_detected": ["nextjs", "nestjs", "nuxt", "sveltekit", "astro", "remix", "vite", "plain_node", "unknown"],
            "runtime_targets": ["node", "browser"],
            "package_manager": ["npm", "pnpm", "yarn", "bun", null]
        },
        "warnings": {
            "No runtime workspace members were detected": "The root appears to be a workspace, but no runtime-bearing package was found. The payload emits install commands only.",
            "No local coverage artifact was detected yet": "Run the application with runtime coverage collection enabled, then re-run setup or health with the produced capture path.",
            "Package manager was not detected": "No packageManager field or known lockfile was found. Commands fall back to npm.",
            "Framework was not detected": "No known framework dependency or runtime script was found. Treat the recipe as a generic Node setup and adjust the entry path as needed."
        }
    })
}

/// Build the `_meta` object for `fallow coverage analyze --format json --explain`.
#[must_use]
pub fn coverage_analyze_meta() -> Value {
    json!({
        "docs_url": COVERAGE_ANALYZE_DOCS,
        "field_definitions": {
            "schema_version": "Standalone coverage analyze envelope version. \"1\" for the current shape.",
            "version": "fallow CLI version that produced this output.",
            "elapsed_ms": "Wall-clock milliseconds spent producing the report.",
            "runtime_coverage": "Same RuntimeCoverageReport block emitted by `fallow health --runtime-coverage`.",
            "runtime_coverage.summary.data_source": "Which evidence source produced the report. local = on-disk artifact via --runtime-coverage <path>; cloud = explicit pull via --cloud / --runtime-coverage-cloud / FALLOW_RUNTIME_COVERAGE_SOURCE=cloud.",
            "runtime_coverage.summary.last_received_at": "ISO-8601 timestamp of the newest runtime payload included in the report. Null for local artifacts that do not carry receipt metadata.",
            "runtime_coverage.summary.capture_quality": "Capture-window telemetry derived from the runtime evidence. lazy_parse_warning trips when more than 30% of tracked functions are V8-untracked, which usually indicates a short observation window.",
            "runtime_coverage.findings[].evidence.static_status": "used = the function is reachable in the AST module graph; unused = it is dead by static analysis.",
            "runtime_coverage.findings[].evidence.test_coverage": "covered = the local test suite hits the function; not_covered otherwise.",
            "runtime_coverage.findings[].evidence.v8_tracking": "tracked = V8 observed the function during the capture window; untracked otherwise.",
            "runtime_coverage.findings[].actions[].type": "Suggested follow-up identifier. delete-cold-code is emitted on safe_to_delete; review-runtime on review_required.",
            "runtime_coverage.blast_radius[]": "First-class blast-radius entries with stable fallow:blast IDs, static caller count, traffic-weighted caller reach, optional cloud deploy touch count, and low/medium/high risk band.",
            "runtime_coverage.importance[]": "First-class production-importance entries with stable fallow:importance IDs, invocations, cyclomatic complexity, owner count, 0-100 importance score, and templated reason.",
            "runtime_coverage.warnings[].code": "Stable warning identifier. cloud_functions_unmatched flags entries dropped because no AST/static counterpart was found locally."
        },
        "enums": {
            "data_source": ["local", "cloud"],
            "report_verdict": ["clean", "hot-path-touched", "cold-code-detected", "license-expired-grace", "unknown"],
            "finding_verdict": ["safe_to_delete", "review_required", "coverage_unavailable", "low_traffic", "active", "unknown"],
            "static_status": ["used", "unused"],
            "test_coverage": ["covered", "not_covered"],
            "v8_tracking": ["tracked", "untracked"],
            "action_type": ["delete-cold-code", "review-runtime"]
        },
        "warnings": {
            "no_runtime_data": "Cloud returned an empty runtime window. Either the period is too narrow or no traces have been ingested yet.",
            "cloud_functions_unmatched": "One or more cloud-side functions could not be matched against the local AST/static index and were dropped from findings. Common causes: stale runtime data after a rename/move, file path mismatch between deploy and repo, or analysis run on the wrong commit."
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── rule_by_id ───────────────────────────────────────────────────

    #[test]
    fn rule_by_id_finds_check_rule() {
        let rule = rule_by_id("fallow/unused-file").unwrap();
        assert_eq!(rule.name, "Unused Files");
    }

    #[test]
    fn rule_by_id_finds_health_rule() {
        let rule = rule_by_id("fallow/high-cyclomatic-complexity").unwrap();
        assert_eq!(rule.name, "High Cyclomatic Complexity");
    }

    #[test]
    fn rule_by_id_finds_dupes_rule() {
        let rule = rule_by_id("fallow/code-duplication").unwrap();
        assert_eq!(rule.name, "Code Duplication");
    }

    #[test]
    fn rule_by_id_returns_none_for_unknown() {
        assert!(rule_by_id("fallow/nonexistent").is_none());
        assert!(rule_by_id("").is_none());
    }

    // ── rule_docs_url ────────────────────────────────────────────────

    #[test]
    fn rule_docs_url_format() {
        let rule = rule_by_id("fallow/unused-export").unwrap();
        let url = rule_docs_url(rule);
        assert!(url.starts_with("https://docs.fallow.tools/"));
        assert!(url.contains("unused-exports"));
    }

    // ── CHECK_RULES completeness ─────────────────────────────────────

    #[test]
    fn check_rules_all_have_fallow_prefix() {
        for rule in CHECK_RULES {
            assert!(
                rule.id.starts_with("fallow/"),
                "rule {} should start with fallow/",
                rule.id
            );
        }
    }

    #[test]
    fn check_rules_all_have_docs_path() {
        for rule in CHECK_RULES {
            assert!(
                !rule.docs_path.is_empty(),
                "rule {} should have a docs_path",
                rule.id
            );
        }
    }

    #[test]
    fn check_rules_no_duplicate_ids() {
        let mut seen = rustc_hash::FxHashSet::default();
        for rule in CHECK_RULES.iter().chain(HEALTH_RULES).chain(DUPES_RULES) {
            assert!(seen.insert(rule.id), "duplicate rule id: {}", rule.id);
        }
    }

    // ── check_meta ───────────────────────────────────────────────────

    #[test]
    fn check_meta_has_docs_and_rules() {
        let meta = check_meta();
        assert!(meta.get("docs").is_some());
        assert!(meta.get("rules").is_some());
        let rules = meta["rules"].as_object().unwrap();
        // Verify all 13 rule categories are present (stripped fallow/ prefix)
        assert_eq!(rules.len(), CHECK_RULES.len());
        assert!(rules.contains_key("unused-file"));
        assert!(rules.contains_key("unused-export"));
        assert!(rules.contains_key("unused-type"));
        assert!(rules.contains_key("unused-dependency"));
        assert!(rules.contains_key("unused-dev-dependency"));
        assert!(rules.contains_key("unused-optional-dependency"));
        assert!(rules.contains_key("unused-enum-member"));
        assert!(rules.contains_key("unused-class-member"));
        assert!(rules.contains_key("unresolved-import"));
        assert!(rules.contains_key("unlisted-dependency"));
        assert!(rules.contains_key("duplicate-export"));
        assert!(rules.contains_key("type-only-dependency"));
        assert!(rules.contains_key("circular-dependency"));
    }

    #[test]
    fn check_meta_rule_has_required_fields() {
        let meta = check_meta();
        let rules = meta["rules"].as_object().unwrap();
        for (key, value) in rules {
            assert!(value.get("name").is_some(), "rule {key} missing 'name'");
            assert!(
                value.get("description").is_some(),
                "rule {key} missing 'description'"
            );
            assert!(value.get("docs").is_some(), "rule {key} missing 'docs'");
        }
    }

    // ── health_meta ──────────────────────────────────────────────────

    #[test]
    fn health_meta_has_metrics() {
        let meta = health_meta();
        assert!(meta.get("docs").is_some());
        let metrics = meta["metrics"].as_object().unwrap();
        assert!(metrics.contains_key("cyclomatic"));
        assert!(metrics.contains_key("cognitive"));
        assert!(metrics.contains_key("maintainability_index"));
        assert!(metrics.contains_key("complexity_density"));
        assert!(metrics.contains_key("fan_in"));
        assert!(metrics.contains_key("fan_out"));
    }

    // ── dupes_meta ───────────────────────────────────────────────────

    #[test]
    fn dupes_meta_has_metrics() {
        let meta = dupes_meta();
        assert!(meta.get("docs").is_some());
        let metrics = meta["metrics"].as_object().unwrap();
        assert!(metrics.contains_key("duplication_percentage"));
        assert!(metrics.contains_key("token_count"));
        assert!(metrics.contains_key("clone_groups"));
        assert!(metrics.contains_key("clone_families"));
    }

    // ── coverage_setup_meta ─────────────────────────────────────────

    #[test]
    fn coverage_setup_meta_has_docs_fields_enums_and_warnings() {
        let meta = coverage_setup_meta();
        assert_eq!(meta["docs_url"], COVERAGE_SETUP_DOCS);
        assert!(
            meta["field_definitions"]
                .as_object()
                .unwrap()
                .contains_key("members[]")
        );
        assert!(
            meta["field_definitions"]
                .as_object()
                .unwrap()
                .contains_key("config_written")
        );
        assert!(
            meta["field_definitions"]
                .as_object()
                .unwrap()
                .contains_key("members[].package_manager")
        );
        assert!(
            meta["field_definitions"]
                .as_object()
                .unwrap()
                .contains_key("members[].warnings")
        );
        assert!(
            meta["enums"]
                .as_object()
                .unwrap()
                .contains_key("framework_detected")
        );
        assert!(
            meta["warnings"]
                .as_object()
                .unwrap()
                .contains_key("No runtime workspace members were detected")
        );
        assert!(
            meta["warnings"]
                .as_object()
                .unwrap()
                .contains_key("Package manager was not detected")
        );
    }

    // ── coverage_analyze_meta ────────────────────────────────────────

    #[test]
    fn coverage_analyze_meta_documents_data_source_and_action_vocabulary() {
        let meta = coverage_analyze_meta();
        assert_eq!(meta["docs_url"], COVERAGE_ANALYZE_DOCS);
        let fields = meta["field_definitions"].as_object().unwrap();
        assert!(fields.contains_key("runtime_coverage.summary.data_source"));
        assert!(fields.contains_key("runtime_coverage.summary.last_received_at"));
        assert!(fields.contains_key("runtime_coverage.findings[].evidence.test_coverage"));
        assert!(fields.contains_key("runtime_coverage.findings[].actions[].type"));
        let enums = meta["enums"].as_object().unwrap();
        assert_eq!(enums["data_source"], json!(["local", "cloud"]));
        assert_eq!(enums["test_coverage"], json!(["covered", "not_covered"]));
        assert_eq!(enums["v8_tracking"], json!(["tracked", "untracked"]));
        assert_eq!(
            enums["action_type"],
            json!(["delete-cold-code", "review-runtime"])
        );
        let warnings = meta["warnings"].as_object().unwrap();
        assert!(warnings.contains_key("cloud_functions_unmatched"));
    }

    // ── HEALTH_RULES completeness ──────────────────────────────────

    #[test]
    fn health_rules_all_have_fallow_prefix() {
        for rule in HEALTH_RULES {
            assert!(
                rule.id.starts_with("fallow/"),
                "health rule {} should start with fallow/",
                rule.id
            );
        }
    }

    #[test]
    fn health_rules_all_have_docs_path() {
        for rule in HEALTH_RULES {
            assert!(
                !rule.docs_path.is_empty(),
                "health rule {} should have a docs_path",
                rule.id
            );
        }
    }

    #[test]
    fn health_rules_all_have_non_empty_fields() {
        for rule in HEALTH_RULES {
            assert!(
                !rule.name.is_empty(),
                "health rule {} missing name",
                rule.id
            );
            assert!(
                !rule.short.is_empty(),
                "health rule {} missing short description",
                rule.id
            );
            assert!(
                !rule.full.is_empty(),
                "health rule {} missing full description",
                rule.id
            );
        }
    }

    // ── DUPES_RULES completeness ───────────────────────────────────

    #[test]
    fn dupes_rules_all_have_fallow_prefix() {
        for rule in DUPES_RULES {
            assert!(
                rule.id.starts_with("fallow/"),
                "dupes rule {} should start with fallow/",
                rule.id
            );
        }
    }

    #[test]
    fn dupes_rules_all_have_docs_path() {
        for rule in DUPES_RULES {
            assert!(
                !rule.docs_path.is_empty(),
                "dupes rule {} should have a docs_path",
                rule.id
            );
        }
    }

    #[test]
    fn dupes_rules_all_have_non_empty_fields() {
        for rule in DUPES_RULES {
            assert!(!rule.name.is_empty(), "dupes rule {} missing name", rule.id);
            assert!(
                !rule.short.is_empty(),
                "dupes rule {} missing short description",
                rule.id
            );
            assert!(
                !rule.full.is_empty(),
                "dupes rule {} missing full description",
                rule.id
            );
        }
    }

    // ── CHECK_RULES field completeness ─────────────────────────────

    #[test]
    fn check_rules_all_have_non_empty_fields() {
        for rule in CHECK_RULES {
            assert!(!rule.name.is_empty(), "check rule {} missing name", rule.id);
            assert!(
                !rule.short.is_empty(),
                "check rule {} missing short description",
                rule.id
            );
            assert!(
                !rule.full.is_empty(),
                "check rule {} missing full description",
                rule.id
            );
        }
    }

    // ── rule_docs_url with health/dupes rules ──────────────────────

    #[test]
    fn rule_docs_url_health_rule() {
        let rule = rule_by_id("fallow/high-cyclomatic-complexity").unwrap();
        let url = rule_docs_url(rule);
        assert!(url.starts_with("https://docs.fallow.tools/"));
        assert!(url.contains("health"));
    }

    #[test]
    fn rule_docs_url_dupes_rule() {
        let rule = rule_by_id("fallow/code-duplication").unwrap();
        let url = rule_docs_url(rule);
        assert!(url.starts_with("https://docs.fallow.tools/"));
        assert!(url.contains("duplication"));
    }

    // ── health_meta metric structure ───────────────────────────────

    #[test]
    fn health_meta_all_metrics_have_name_and_description() {
        let meta = health_meta();
        let metrics = meta["metrics"].as_object().unwrap();
        for (key, value) in metrics {
            assert!(
                value.get("name").is_some(),
                "health metric {key} missing 'name'"
            );
            assert!(
                value.get("description").is_some(),
                "health metric {key} missing 'description'"
            );
            assert!(
                value.get("interpretation").is_some(),
                "health metric {key} missing 'interpretation'"
            );
        }
    }

    #[test]
    fn health_meta_has_all_expected_metrics() {
        let meta = health_meta();
        let metrics = meta["metrics"].as_object().unwrap();
        let expected = [
            "cyclomatic",
            "cognitive",
            "line_count",
            "lines",
            "maintainability_index",
            "complexity_density",
            "dead_code_ratio",
            "fan_in",
            "fan_out",
            "score",
            "weighted_commits",
            "trend",
            "priority",
            "efficiency",
            "effort",
            "confidence",
            "bus_factor",
            "contributor_count",
            "share",
            "stale_days",
            "drift",
            "unowned",
            "runtime_coverage_verdict",
            "runtime_coverage_state",
            "runtime_coverage_confidence",
            "production_invocations",
            "percent_dead_in_production",
        ];
        for key in &expected {
            assert!(
                metrics.contains_key(*key),
                "health_meta missing expected metric: {key}"
            );
        }
    }

    // ── dupes_meta metric structure ────────────────────────────────

    #[test]
    fn dupes_meta_all_metrics_have_name_and_description() {
        let meta = dupes_meta();
        let metrics = meta["metrics"].as_object().unwrap();
        for (key, value) in metrics {
            assert!(
                value.get("name").is_some(),
                "dupes metric {key} missing 'name'"
            );
            assert!(
                value.get("description").is_some(),
                "dupes metric {key} missing 'description'"
            );
        }
    }

    #[test]
    fn dupes_meta_has_line_count() {
        let meta = dupes_meta();
        let metrics = meta["metrics"].as_object().unwrap();
        assert!(metrics.contains_key("line_count"));
    }

    // ── docs URLs ─────────────────────────────────────────────────

    #[test]
    fn check_docs_url_valid() {
        assert!(CHECK_DOCS.starts_with("https://"));
        assert!(CHECK_DOCS.contains("dead-code"));
    }

    #[test]
    fn health_docs_url_valid() {
        assert!(HEALTH_DOCS.starts_with("https://"));
        assert!(HEALTH_DOCS.contains("health"));
    }

    #[test]
    fn dupes_docs_url_valid() {
        assert!(DUPES_DOCS.starts_with("https://"));
        assert!(DUPES_DOCS.contains("dupes"));
    }

    // ── check_meta docs URL matches constant ──────────────────────

    #[test]
    fn check_meta_docs_url_matches_constant() {
        let meta = check_meta();
        assert_eq!(meta["docs"].as_str().unwrap(), CHECK_DOCS);
    }

    #[test]
    fn health_meta_docs_url_matches_constant() {
        let meta = health_meta();
        assert_eq!(meta["docs"].as_str().unwrap(), HEALTH_DOCS);
    }

    #[test]
    fn dupes_meta_docs_url_matches_constant() {
        let meta = dupes_meta();
        assert_eq!(meta["docs"].as_str().unwrap(), DUPES_DOCS);
    }

    // ── rule_by_id finds all check rules ──────────────────────────

    #[test]
    fn rule_by_id_finds_all_check_rules() {
        for rule in CHECK_RULES {
            assert!(
                rule_by_id(rule.id).is_some(),
                "rule_by_id should find check rule {}",
                rule.id
            );
        }
    }

    #[test]
    fn rule_by_id_finds_all_health_rules() {
        for rule in HEALTH_RULES {
            assert!(
                rule_by_id(rule.id).is_some(),
                "rule_by_id should find health rule {}",
                rule.id
            );
        }
    }

    #[test]
    fn rule_by_id_finds_all_dupes_rules() {
        for rule in DUPES_RULES {
            assert!(
                rule_by_id(rule.id).is_some(),
                "rule_by_id should find dupes rule {}",
                rule.id
            );
        }
    }

    // ── Rule count verification ───────────────────────────────────

    #[test]
    fn check_rules_count() {
        assert_eq!(CHECK_RULES.len(), 17);
    }

    #[test]
    fn health_rules_count() {
        assert_eq!(HEALTH_RULES.len(), 12);
    }

    #[test]
    fn dupes_rules_count() {
        assert_eq!(DUPES_RULES.len(), 1);
    }

    /// Every registered rule must declare a category. The PR/MR sticky
    /// renderer reads this via `category_for_rule`; without an entry the
    /// rule silently falls into the "Dead code" default and reviewers may
    /// see it grouped under an unexpected section. Catching this here is
    /// the same pattern as `check_rules_count` for the rule count itself.
    #[test]
    fn every_rule_declares_a_category() {
        let allowed = [
            "Dead code",
            "Dependencies",
            "Duplication",
            "Health",
            "Architecture",
            "Suppressions",
        ];
        for rule in CHECK_RULES.iter().chain(HEALTH_RULES).chain(DUPES_RULES) {
            assert!(
                !rule.category.is_empty(),
                "rule {} has empty category",
                rule.id
            );
            assert!(
                allowed.contains(&rule.category),
                "rule {} has unrecognised category {:?}; add to allowlist or pick from {:?}",
                rule.id,
                rule.category,
                allowed
            );
        }
    }
}
