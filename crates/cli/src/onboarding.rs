//! Agent onboarding surface shared by the capability manifest (`fallow schema`)
//! and the `fallow recommend` command.
//!
//! Two catalogs live here so both surfaces read the SAME data and can never
//! drift: the built-in boundary presets and the curated taste-choice catalog.
//!
//! The taste-choice catalog encodes fallow's three-valued decision model:
//!
//! - `auto`: decided from detection and applied silently. Auto decisions are NOT
//!   in this catalog because they are not choices the user is offered; they are
//!   emitted per-project by `fallow recommend`.
//! - `default` (this catalog, `Tier::Default`): fallow ships a defensible default,
//!   disclosed and overridable, carrying a quantified rationale. NOT asked on a
//!   first run unless the project already declares a matching tool to migrate.
//! - `taste` (this catalog, `Tier::Taste`): a genuinely subjective choice with
//!   symmetric tradeoffs and NO recommended answer baked into the question. These
//!   are the ~2-3 choices surfaced on a first run.

use fallow_config::BoundaryPreset;

/// Which decision tier a subjective knob belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Disclosed default, overridable, not asked on a first run.
    Default,
    /// Genuinely subjective; surfaced as an open question with no recommendation.
    Taste,
}

impl Tier {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Taste => "taste",
        }
    }
}

/// One selectable option for a `Taste`-tier choice. Deliberately carries NO
/// "recommended" marker: a true taste choice must not smuggle an answer into the
/// question (fallow taste-ownership principle).
#[derive(Debug, Clone, Copy)]
pub struct TasteOption {
    /// Short human label (feeds `AskUserQuestion` option labels).
    pub label: &'static str,
    /// The concrete config effect this option applies, in words.
    pub effect: &'static str,
    /// The tradeoff a team accepts by picking this option.
    pub tradeoff: &'static str,
}

/// A subjective knob an onboarding agent should know about.
#[derive(Debug, Clone, Copy)]
pub struct TasteChoice {
    /// Stable id (kebab-case), safe to branch on.
    pub id: &'static str,
    /// Short human title, `AskUserQuestion`-header-safe (<= 12 chars where it
    /// is used as a header; the full title may be longer for docs).
    pub header: &'static str,
    /// The full question prompt (open phrasing for `Taste`, descriptive for `Default`).
    pub prompt: &'static str,
    /// Decision tier.
    pub tier: Tier,
    /// Whether this is surfaced in the first-run onboarding ask.
    pub first_run: bool,
    /// Whether the knob is only relevant when a matching framework is detected.
    pub framework_gated: bool,
    /// The config paths this knob controls. `rules.<name>` paths reference a
    /// real rule name; a drift test pins every `rules.*` entry to a known rule.
    pub config_paths: &'static [&'static str],
    /// The zero-config default, in words (the quantified rationale for `Default`).
    pub default_summary: &'static str,
    /// The options an agent presents for a `Taste` choice (empty for `Default`).
    pub options: &'static [TasteOption],
}

/// The curated taste-choice catalog.
///
/// Pruned per panel review to avoid over-asking: only `ci-strictness` and
/// `private-type-leak` are `first_run` taste questions. Everything else is a
/// disclosed `Default` an agent can mention but should not interrogate the user
/// about on a cold start.
pub const TASTE_CHOICES: &[TasteChoice] = &[
    TasteChoice {
        id: "ci-strictness",
        header: "CI gate",
        prompt: "Should the cleanup rules that default to warn (unused dev/optional \
                 dependencies, component-level dead code, styling drift) fail CI, or stay \
                 advisory?",
        tier: Tier::Taste,
        first_run: true,
        framework_gated: false,
        config_paths: &[
            "rules.unused-dev-dependencies",
            "rules.unused-optional-dependencies",
            "rules.stale-suppressions",
        ],
        default_summary: "These rules default to warn: reported, never failing CI. The \
                          structural rules (unused files/exports/deps, circular deps, \
                          unresolved imports, boundary violations) already default to error.",
        options: &[
            TasteOption {
                label: "Advisory",
                effect: "Keep the warn-default rules at warn.",
                tradeoff: "Cleanup drift is visible but never blocks a merge; the team \
                           addresses it on its own cadence.",
            },
            TasteOption {
                label: "Strict",
                effect: "Promote the warn-default cleanup rules to error.",
                tradeoff: "A PR that adds unused dev deps or dead component surface fails \
                           CI; noisier on legacy code, tighter on new code.",
            },
        ],
    },
    TasteChoice {
        id: "private-type-leak",
        header: "API hygiene",
        prompt: "Enable the opt-in private-type-leak check, which flags exported \
                 signatures that reference a same-file private type (a break for consumers)?",
        tier: Tier::Taste,
        first_run: true,
        framework_gated: false,
        config_paths: &["rules.private-type-leaks"],
        default_summary: "Off by default: it is a lower-confidence API-hygiene check, so \
                          fallow does not enable it unless you opt in.",
        options: &[
            TasteOption {
                label: "Keep off",
                effect: "Leave private-type-leak disabled.",
                tradeoff: "No API-hygiene findings; a leaked private type in a public \
                           signature goes unflagged.",
            },
            TasteOption {
                label: "Enable (warn)",
                effect: "Set rules.private-type-leaks to warn.",
                tradeoff: "Catches public signatures that reference private types; can be \
                           noisy on libraries with intentional internal-type re-use.",
            },
        ],
    },
    TasteChoice {
        id: "dupes-sensitivity",
        header: "Dupes",
        prompt: "Code-duplication sensitivity (how large and how repeated a clone must be \
                 before it is reported).",
        tier: Tier::Default,
        first_run: false,
        framework_gated: false,
        config_paths: &[
            "duplicates.mode",
            "duplicates.minTokens",
            "duplicates.minOccurrences",
        ],
        default_summary: "mild mode, minTokens 50, minOccurrences 2: reports moderately \
                          sized clones that appear at least twice. Raise minOccurrences to 3 \
                          to focus on widespread duplication only. Note: `fallow init`'s \
                          starter config seeds minOccurrences at 3 to hide pair-only noise on \
                          a first run, so its output is intentionally stricter than this \
                          zero-config default of 2.",
        options: &[],
    },
    TasteChoice {
        id: "complexity-ceilings",
        header: "Complexity",
        prompt: "Function complexity ceilings used by fallow health.",
        tier: Tier::Default,
        first_run: false,
        framework_gated: false,
        config_paths: &[
            "health.maxCyclomaticComplexity",
            "health.maxCognitiveComplexity",
            "health.maxCrapThreshold",
            "health.maxUnitSize",
        ],
        default_summary: "cyclomatic 20, cognitive 15, CRAP 30, unit size 60 lines. These \
                          are SIG-aligned; raise them for an established codebase that would \
                          otherwise report a large existing backlog.",
        options: &[],
    },
    TasteChoice {
        id: "styling-drift",
        header: "Styling",
        prompt: "Design-system styling-drift strictness (hardcoded values where a token \
                 exists, duplicate blocks, selector complexity).",
        tier: Tier::Default,
        first_run: false,
        framework_gated: false,
        config_paths: &[
            "rules.css-token-drift",
            "rules.css-duplicate-block",
            "rules.css-selector-complexity",
        ],
        default_summary: "All css-* rules default to warn (advisory, verdict-neutral). \
                          Promote to error only once the design system is stable enough that \
                          drift should block a merge.",
        options: &[],
    },
    TasteChoice {
        id: "react-health-signals",
        header: "React heur",
        prompt: "React/Preact structural health signals (prop-drilling, thin-wrapper, \
                 duplicate-prop-shape).",
        tier: Tier::Default,
        first_run: false,
        framework_gated: true,
        config_paths: &[
            "rules.prop-drilling",
            "rules.thin-wrapper",
            "rules.duplicate-prop-shape",
        ],
        default_summary: "Off by default: opinion-heavy heuristics that are noisy on \
                          established component trees. Enable per-rule (warn) only if the team \
                          wants refactor pressure on component structure.",
        options: &[],
    },
    TasteChoice {
        id: "coverage-gaps",
        header: "Cov gaps",
        prompt: "Coverage-gap detection (runtime files/exports with no test dependency path).",
        tier: Tier::Default,
        first_run: false,
        framework_gated: false,
        config_paths: &["rules.coverage-gaps"],
        default_summary: "Off by default and most useful once runtime coverage data is \
                          available. Not surfaced on a first run because a cold project rarely \
                          has the inputs it needs.",
        options: &[],
    },
    TasteChoice {
        id: "security",
        header: "Security",
        prompt: "Opt-in security candidate detection (client/server secret leaks, tainted \
                 sinks). Surfaced by the separate fallow security command.",
        tier: Tier::Default,
        first_run: false,
        framework_gated: false,
        config_paths: &["rules.security-client-server-leak", "rules.security-sink"],
        default_summary: "Off by default: candidates are unverified and high-noise, so \
                          enabling security is a deliberate decision, not a first-run default. \
                          Run fallow security to explore candidates without changing config.",
        options: &[],
    },
];

/// Build the `boundary_presets` manifest section from the canonical enum.
pub fn boundary_presets_schema() -> serde_json::Value {
    let presets: Vec<serde_json::Value> = BoundaryPreset::all()
        .iter()
        .map(|preset| {
            serde_json::json!({
                "name": preset.name(),
                "description": preset.description(),
                "config": format!("boundaries.preset: \"{}\"", preset.name()),
            })
        })
        .collect();
    serde_json::json!(presets)
}

/// Build the `taste_choices` manifest section from the curated catalog.
pub fn taste_choices_schema() -> serde_json::Value {
    let choices: Vec<serde_json::Value> = TASTE_CHOICES
        .iter()
        .map(|choice| {
            let options: Vec<serde_json::Value> = choice
                .options
                .iter()
                .map(|opt| {
                    serde_json::json!({
                        "label": opt.label,
                        "effect": opt.effect,
                        "tradeoff": opt.tradeoff,
                    })
                })
                .collect();
            serde_json::json!({
                "id": choice.id,
                "header": choice.header,
                "prompt": choice.prompt,
                "tier": choice.tier.as_str(),
                "first_run": choice.first_run,
                "framework_gated": choice.framework_gated,
                "config_paths": choice.config_paths,
                "default_summary": choice.default_summary,
                "options": options,
            })
        })
        .collect();
    serde_json::json!(choices)
}

// ---------------------------------------------------------------------------
// `fallow recommend`: project-tailored config recommendation
// ---------------------------------------------------------------------------

use std::path::Path;
use std::process::ExitCode;

use fallow_config::{OutputFormat, PackageJson};

use crate::init::{ProjectInfo, detect_project};

/// Framework presence signals: a framework label plus the dependency names that
/// indicate it is in use. Used to report which framework-gated rules will
/// auto-activate (fallow's detectors self-gate on these, so `recommend` never
/// needs to write their severities into the config).
const FRAMEWORK_SIGNALS: &[(&str, &[&str])] = &[
    ("next", &["next"]),
    ("react", &["react", "react-dom", "preact"]),
    ("vue", &["vue"]),
    ("svelte", &["svelte"]),
    ("angular", &["@angular/core"]),
];

/// UI frameworks (a project using more than one across a monorepo is
/// heterogeneous, so framework rules must not be uniformly assumed).
const UI_FRAMEWORKS: &[&str] = &["react", "vue", "svelte", "angular"];

/// Which of the three decision tiers a recommendation entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionKind {
    /// Decided from detection, applied silently (still disclosed in the output).
    Auto,
    /// A defensible default, disclosed and overridable, carrying a rationale.
    Default,
    /// A genuinely subjective choice surfaced to the user as an open question.
    Taste,
}

impl DecisionKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Default => "default",
            Self::Taste => "taste",
        }
    }
}

/// One entry in the recommendation's decision list.
pub struct Decision {
    /// What is being decided (a config path or a capability name).
    pub setting: String,
    /// The recommended value, or `Null` for an informational decision.
    pub value: serde_json::Value,
    /// Why, quantified where possible.
    pub rationale: String,
    /// The decision tier.
    pub kind: DecisionKind,
    /// For `Taste` decisions, an `AskUserQuestion`-shaped payload the agent
    /// presents to the user. `None` for `Auto`/`Default`.
    pub question: Option<serde_json::Value>,
}

/// A project-tailored config recommendation: what fallow detected, a proposed
/// config it can write, and the per-setting decisions (auto/default/taste).
pub struct Recommendation {
    detected: serde_json::Value,
    proposed_config: serde_json::Value,
    decisions: Vec<Decision>,
}

impl Recommendation {
    /// The recommendation as the `fallow recommend --format json` envelope.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let decisions: Vec<serde_json::Value> = self
            .decisions
            .iter()
            .map(|d| {
                serde_json::json!({
                    "setting": d.setting,
                    "value": d.value,
                    "rationale": d.rationale,
                    "kind": d.kind.as_str(),
                    "question": d.question,
                })
            })
            .collect();
        serde_json::json!({
            "kind": "recommendation",
            "schema_version": "1",
            "detected": self.detected,
            "proposed_config": self.proposed_config,
            "decisions": decisions,
            "note": "proposed_config is the safe, detection-derived baseline; resolve every \
                     taste decision with the user before finalizing. For the full set of valid \
                     config keys and their shapes, run `fallow config-schema`. Zero config is a \
                     valid stop: fallow's defaults are strong, so writing no config at all is \
                     fully supported.",
            "config_schema_command": "fallow config-schema",
        })
    }
}

/// The entry-point glob extensions for a project.
fn entry_extensions(info: &ProjectInfo) -> &'static str {
    if info.has_typescript {
        "{ts,tsx,js,jsx}"
    } else {
        "{js,jsx,mjs}"
    }
}

/// Local schema path for npm consumers: version-aligned with the installed
/// `fallow` package, works offline, and does not trigger VS Code's
/// untrusted-remote-schema prompt (issue #1794). Only emitted when
/// `ProjectInfo::has_local_schema` confirms the file actually exists.
pub const LOCAL_SCHEMA_PATH: &str = "./node_modules/fallow/schema.json";

/// Remote schema fallback for installs with no local `node_modules/fallow`
/// (cargo, homebrew, or a bare binary).
pub const REMOTE_SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json";

/// Build the safe, detection-derived proposed config (pure; no filesystem).
///
/// Only settings fallow can decide with confidence go here: entry points (the
/// `src/index`/`src/main` convention), workspace packages for a monorepo, and a
/// Storybook ignore. Framework rule severities are deliberately NOT written:
/// fallow's detectors self-gate on the framework, so the rules auto-activate at
/// their defaults when the framework is present, and writing them would risk a
/// wrong uniform assumption on a heterogeneous monorepo (panel finding).
pub fn proposed_config_value(info: &ProjectInfo) -> serde_json::Value {
    let ext = entry_extensions(info);
    let schema_url = if info.has_local_schema {
        LOCAL_SCHEMA_PATH
    } else {
        REMOTE_SCHEMA_URL
    };
    let mut config = serde_json::json!({
        "$schema": schema_url,
        "entry": [format!("src/index.{ext}"), format!("src/main.{ext}")],
    });
    if info.is_monorepo && !info.workspace_patterns.is_empty() {
        // The loader's WorkspaceConfig field is `patterns`; `packages` is
        // silently dropped (unknown keys are ignored), so scoping would be lost.
        config["workspaces"] = serde_json::json!({ "patterns": info.workspace_patterns });
    }
    if info.has_storybook {
        config["ignorePatterns"] = serde_json::json!([".storybook/**"]);
    }
    config
}

/// The `AskUserQuestion`-shaped payload for a taste choice.
fn taste_question_json(choice: &TasteChoice) -> serde_json::Value {
    let options: Vec<serde_json::Value> = choice
        .options
        .iter()
        .map(|opt| {
            serde_json::json!({
                "label": opt.label,
                "description": format!("{} {}", opt.effect, opt.tradeoff),
            })
        })
        .collect();
    serde_json::json!({
        "header": choice.header,
        "question": choice.prompt,
        "options": options,
    })
}

/// Build a project-tailored config recommendation by inspecting `root`.
#[must_use]
pub fn build_recommendation(root: &Path) -> Recommendation {
    let info = detect_project(root);
    // Aggregate framework signals across workspace members (not just the root
    // package.json) so a monorepo's frameworks surface in `frameworks_present`.
    let root_pkg = PackageJson::load(&root.join("package.json")).ok();
    let deps = crate::init::collect_dependency_names(root, root_pkg.as_ref(), info.is_monorepo);
    build_recommendation_from(&info, &deps)
}

/// Pure recommendation builder (testable without a real project directory).
fn build_recommendation_from(info: &ProjectInfo, deps: &[String]) -> Recommendation {
    let present: Vec<&'static str> = FRAMEWORK_SIGNALS
        .iter()
        .filter(|(_, packages)| packages.iter().any(|pkg| deps.iter().any(|d| d == pkg)))
        .map(|(label, _)| *label)
        .collect();
    let ui_count = UI_FRAMEWORKS.iter().filter(|f| present.contains(f)).count();
    let heterogeneous = info.is_monorepo && ui_count > 1;

    let proposed_config = proposed_config_value(info);
    let mut decisions = Vec::new();

    // Auto: entry points (names the convention assumption, per panel).
    decisions.push(Decision {
        setting: "entry".to_owned(),
        value: proposed_config["entry"].clone(),
        rationale: "Assumes the src/index or src/main entry convention. Adjust for \
                    framework-routed apps or DI-wired entry points (fallow also auto-honors \
                    package.json exports/main/module for libraries)."
            .to_owned(),
        kind: DecisionKind::Auto,
        question: None,
    });
    if info.is_monorepo && !info.workspace_patterns.is_empty() {
        decisions.push(Decision {
            setting: "workspaces.patterns".to_owned(),
            value: serde_json::json!(info.workspace_patterns),
            rationale: format!(
                "Detected a {} monorepo; scoped analysis to the workspace packages.",
                info.workspace_tool.as_deref().unwrap_or("workspace")
            ),
            kind: DecisionKind::Auto,
            question: None,
        });
    }
    if info.has_storybook {
        decisions.push(Decision {
            setting: "ignorePatterns".to_owned(),
            value: serde_json::json!([".storybook/**"]),
            rationale: "Detected a .storybook directory; excluded it from analysis.".to_owned(),
            kind: DecisionKind::Auto,
            question: None,
        });
    }

    // Auto (informational): framework-gated rules that auto-activate.
    if !present.is_empty() {
        let rationale = if heterogeneous {
            "Multiple UI frameworks detected across the monorepo. Framework-gated rules \
             activate per-package where their framework is present; do NOT assume one \
             framework's rules apply repo-wide."
                .to_owned()
        } else {
            format!(
                "Detected {}. fallow's framework-gated rules (e.g. the Next.js route and \
                 client/server rules) activate automatically at their default severities \
                 when the framework is present; no config needed.",
                present.join(", ")
            )
        };
        decisions.push(Decision {
            setting: "framework-rules".to_owned(),
            value: serde_json::json!(present),
            rationale,
            kind: DecisionKind::Auto,
            question: None,
        });
    }

    // Default + Taste tiers from the shared catalog.
    for choice in TASTE_CHOICES {
        if choice.framework_gated {
            // Only surface a framework-gated knob when its framework is present.
            let relevant = match choice.id {
                "react-health-signals" => present.contains(&"react"),
                _ => true,
            };
            if !relevant {
                continue;
            }
        }
        match choice.tier {
            Tier::Default => decisions.push(Decision {
                setting: choice
                    .config_paths
                    .first()
                    .map_or(choice.id, |p| p)
                    .to_owned(),
                value: serde_json::Value::Null,
                rationale: choice.default_summary.to_owned(),
                kind: DecisionKind::Default,
                question: None,
            }),
            Tier::Taste if choice.first_run => decisions.push(Decision {
                setting: choice.id.to_owned(),
                value: serde_json::Value::Null,
                rationale: choice.default_summary.to_owned(),
                kind: DecisionKind::Taste,
                question: Some(taste_question_json(choice)),
            }),
            Tier::Taste => {}
        }
    }

    let detected = serde_json::json!({
        "is_monorepo": info.is_monorepo,
        "workspace_tool": info.workspace_tool,
        "workspace_patterns": info.workspace_patterns,
        "has_typescript": info.has_typescript,
        "test_framework": info.test_framework,
        "test_framework_ambiguous": info.test_framework_ambiguous,
        "ui_framework": info.ui_framework,
        "frameworks_present": present,
        "heterogeneous_frameworks": heterogeneous,
        "has_storybook": info.has_storybook,
        "package_manager": info.package_manager,
    });

    Recommendation {
        detected,
        proposed_config,
        decisions,
    }
}

/// Run `fallow recommend`: emit a project-tailored config recommendation.
///
/// Read-only and always exits 0 (advisory). JSON is the machine contract; human
/// output is a concise summary that ends by naming the taste questions to ask.
pub fn run_recommend(
    root: &Path,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let recommendation = build_recommendation(root);
    if matches!(output, OutputFormat::Human) {
        print_recommendation_human(&recommendation);
        return ExitCode::SUCCESS;
    }
    match render_recommendation_json(&recommendation, json_style) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => crate::error::emit_error_with_style(
            &format!("failed to serialize recommendation: {e}"),
            2,
            output,
            json_style,
        ),
    }
}

fn render_recommendation_json(
    recommendation: &Recommendation,
    json_style: crate::json_style::JsonStyle,
) -> Result<String, serde_json::Error> {
    json_style.serialize(&recommendation.to_json())
}

/// Concise human summary of a recommendation.
fn print_recommendation_human(rec: &Recommendation) {
    print!("{}", recommendation_human_report(rec));
}

/// Build the concise human summary of a recommendation as a string (so the
/// content, including the `--format json` pointer, is unit-testable).
///
/// The human view is deliberately concise: it shows the detected frameworks, the
/// proposed config, and the taste questions to ask, then points at
/// `--format json` for the full structured decision set (the `detected` block,
/// every decision's tier and rationale, and each taste question's options),
/// which an agent consumes rather than the human text.
fn recommendation_human_report(rec: &Recommendation) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let frameworks = rec.detected["frameworks_present"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "none detected".to_owned());
    let _ = writeln!(out, "Detected frameworks: {frameworks}");
    if let Ok(config) = serde_json::to_string_pretty(&rec.proposed_config) {
        let _ = writeln!(out, "\nProposed config (a safe starting point):\n{config}");
    }

    let taste: Vec<&Decision> = rec
        .decisions
        .iter()
        .filter(|d| d.kind == DecisionKind::Taste)
        .collect();
    if taste.is_empty() {
        let _ = writeln!(
            out,
            "\nNo taste questions: the detection-derived config plus fallow's strong defaults cover this project."
        );
    } else {
        let _ = writeln!(
            out,
            "\nAsk the user these {} taste question(s):",
            taste.len()
        );
        for d in taste {
            if let Some(q) = &d.question {
                let _ = writeln!(out, "  - {}", q["question"].as_str().unwrap_or(&d.setting));
            }
        }
    }
    let _ = writeln!(
        out,
        "\nZero config is a valid stop: fallow's defaults are strong, so writing no config is fully supported."
    );
    let _ = writeln!(
        out,
        "\nTip: run `fallow recommend --format json` for the full structured decision set: the detected project shape, every decision's auto/default/taste tier and rationale, and the AskUserQuestion-shaped options behind each taste question."
    );
    out
}

#[cfg(test)]
mod tests {
    use fallow_config::KNOWN_RULE_NAMES;

    use super::*;

    /// The rule name embedded in a `rules.<name>` config path, if any.
    fn rules_path_name(path: &str) -> Option<&str> {
        path.strip_prefix("rules.")
    }

    #[test]
    fn every_rules_config_path_names_a_known_rule() {
        for choice in TASTE_CHOICES {
            for path in choice.config_paths {
                if let Some(name) = rules_path_name(path) {
                    assert!(
                        KNOWN_RULE_NAMES.contains(&name),
                        "taste choice '{}' references rules.{name}, which is not a known rule",
                        choice.id
                    );
                }
            }
        }
    }

    #[test]
    fn taste_choice_ids_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for choice in TASTE_CHOICES {
            assert!(
                seen.insert(choice.id),
                "duplicate taste choice id: {}",
                choice.id
            );
        }
    }

    #[test]
    fn taste_tier_choices_carry_options_and_default_tier_do_not() {
        for choice in TASTE_CHOICES {
            match choice.tier {
                Tier::Taste => assert!(
                    choice.options.len() >= 2,
                    "taste-tier choice '{}' needs >= 2 options",
                    choice.id
                ),
                Tier::Default => assert!(
                    choice.options.is_empty(),
                    "default-tier choice '{}' must not carry options (it is not asked)",
                    choice.id
                ),
            }
        }
    }

    #[test]
    fn first_run_choices_are_taste_tier_and_capped() {
        let first_run: Vec<_> = TASTE_CHOICES.iter().filter(|c| c.first_run).collect();
        assert!(
            first_run.len() <= 4,
            "first-run taste questions must stay capped (<= 4) to avoid onboarding fatigue"
        );
        for choice in first_run {
            assert_eq!(
                choice.tier,
                Tier::Taste,
                "first-run choice '{}' must be a taste-tier open question",
                choice.id
            );
        }
    }

    #[test]
    fn boundary_presets_schema_lists_every_preset() {
        let schema = boundary_presets_schema();
        let names: Vec<&str> = schema
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        for preset in BoundaryPreset::all() {
            assert!(
                names.contains(&preset.name()),
                "boundary_presets_schema omitted {}",
                preset.name()
            );
        }
    }

    fn ts_lib_info() -> ProjectInfo {
        ProjectInfo {
            is_monorepo: false,
            workspace_patterns: Vec::new(),
            workspace_tool: None,
            has_typescript: true,
            test_framework: Some("Vitest".to_owned()),
            ui_framework: None,
            has_storybook: false,
            package_manager: Some("pnpm".to_owned()),
            test_framework_ambiguous: false,
            has_local_schema: false,
        }
    }

    fn next_monorepo_info() -> ProjectInfo {
        ProjectInfo {
            is_monorepo: true,
            workspace_patterns: vec!["apps/*".to_owned(), "packages/*".to_owned()],
            workspace_tool: Some("pnpm".to_owned()),
            has_typescript: true,
            test_framework: Some("Vitest".to_owned()),
            ui_framework: Some("React".to_owned()),
            has_storybook: true,
            package_manager: Some("pnpm".to_owned()),
            test_framework_ambiguous: false,
            has_local_schema: false,
        }
    }

    /// The crux: the proposed config must LOAD through the real config type, not
    /// merely be plausible JSON (panel: schema-valid is not loader-accepted). AND
    /// the workspace patterns must SURVIVE the round-trip: unknown keys are
    /// silently dropped, so a wrong key (e.g. `packages` instead of `patterns`)
    /// would deserialize fine yet lose scoping.
    #[test]
    fn proposed_config_loads_and_preserves_workspaces() {
        for info in [ts_lib_info(), next_monorepo_info()] {
            let config = proposed_config_value(&info);
            let parsed: fallow_config::FallowConfig = serde_json::from_value(config.clone())
                .unwrap_or_else(|e| {
                    panic!("proposed_config must deserialize into FallowConfig: {config:#} -> {e}")
                });
            if info.is_monorepo && !info.workspace_patterns.is_empty() {
                assert_eq!(
                    parsed.workspaces.map(|w| w.patterns).unwrap_or_default(),
                    info.workspace_patterns,
                    "workspace patterns must survive the round-trip (correct config key)"
                );
            }
        }
    }

    /// Issue #1794: when a local `node_modules/fallow/schema.json` is present,
    /// the proposed config must point `$schema` at it (version-aligned, offline,
    /// no VS Code untrusted-remote-schema prompt) rather than the remote URL.
    #[test]
    fn proposed_config_schema_prefers_local_when_available() {
        let mut info = ts_lib_info();
        info.has_local_schema = true;
        let config = proposed_config_value(&info);
        assert_eq!(config["$schema"], LOCAL_SCHEMA_PATH);
    }

    /// Without a detected local schema (cargo, homebrew, or a bare binary
    /// install), the proposed config must fall back to the remote URL rather
    /// than emit a local path that fails to resolve.
    #[test]
    fn proposed_config_schema_falls_back_to_remote_without_local() {
        let mut info = ts_lib_info();
        info.has_local_schema = false;
        let config = proposed_config_value(&info);
        assert_eq!(config["$schema"], REMOTE_SCHEMA_URL);
    }

    /// Framework rule severities are NEVER written into the config; they
    /// auto-activate. So an agent cannot wrongly apply one framework's rules on
    /// a heterogeneous monorepo.
    #[test]
    fn proposed_config_never_writes_framework_rule_severities() {
        let rec = build_recommendation_from(&next_monorepo_info(), &["next".to_owned()]);
        assert!(
            rec.proposed_config.get("rules").is_none(),
            "recommend must not write rule severities into proposed_config"
        );
    }

    #[test]
    fn next_monorepo_reports_framework_autoactivation_and_workspaces() {
        let rec = build_recommendation_from(
            &next_monorepo_info(),
            &["next".to_owned(), "react".to_owned()],
        );
        let json = rec.to_json();
        assert_eq!(json["detected"]["frameworks_present"][0], "next");
        assert_eq!(
            json["proposed_config"]["workspaces"]["patterns"][0],
            "apps/*"
        );
        assert_eq!(
            json["proposed_config"]["ignorePatterns"][0],
            ".storybook/**"
        );
        let settings: Vec<&str> = json["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["setting"].as_str().unwrap())
            .collect();
        assert!(settings.contains(&"framework-rules"));
        assert!(settings.contains(&"workspaces.patterns"));
    }

    /// Exactly the two first-run taste choices surface as taste decisions with a
    /// well-formed AskUserQuestion-shaped payload and NO recommended answer.
    #[test]
    fn first_run_taste_decisions_are_ask_user_question_shaped() {
        let rec = build_recommendation_from(&ts_lib_info(), &[]);
        let json = rec.to_json();
        let taste: Vec<&serde_json::Value> = json["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|d| d["kind"] == "taste")
            .collect();
        let ids: Vec<&str> = taste
            .iter()
            .map(|d| d["setting"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["ci-strictness", "private-type-leak"]);
        for d in taste {
            let q = &d["question"];
            let header = q["header"].as_str().unwrap();
            assert!(
                header.len() <= 12,
                "taste question header '{header}' exceeds AskUserQuestion's 12-char limit"
            );
            let options = q["options"].as_array().unwrap();
            assert!(
                (2..=4).contains(&options.len()),
                "taste question needs 2-4 options, got {}",
                options.len()
            );
            for opt in options {
                assert!(opt["label"].as_str().is_some());
                assert!(opt["description"].as_str().is_some());
            }
        }
    }

    /// A react-gated default is dropped when React is absent (framework-gated
    /// knobs only surface when relevant).
    #[test]
    fn framework_gated_default_dropped_without_framework() {
        let rec = build_recommendation_from(&ts_lib_info(), &[]);
        let json = rec.to_json();
        let settings: Vec<&str> = json["decisions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["setting"].as_str().unwrap())
            .collect();
        assert!(
            !settings.iter().any(|s| s.contains("prop-drilling")),
            "react-health-signals must not surface on a non-React project"
        );
    }

    #[test]
    fn recommendation_is_deterministic() {
        let a = build_recommendation_from(&next_monorepo_info(), &["next".to_owned()]).to_json();
        let b = build_recommendation_from(&next_monorepo_info(), &["next".to_owned()]).to_json();
        assert_eq!(a, b, "recommend output must be byte-deterministic");
    }

    #[test]
    fn recommendation_json_respects_explicit_style() {
        let recommendation = build_recommendation_from(&ts_lib_info(), &[]);
        let compact =
            render_recommendation_json(&recommendation, crate::json_style::JsonStyle::Compact)
                .expect("compact recommendation should serialize");
        let pretty =
            render_recommendation_json(&recommendation, crate::json_style::JsonStyle::Pretty)
                .expect("pretty recommendation should serialize");

        assert!(
            !compact.contains('\n'),
            "compact JSON must stay on one line"
        );
        assert!(pretty.contains("\n  \""), "pretty JSON must be indented");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&compact).unwrap(),
            serde_json::from_str::<serde_json::Value>(&pretty).unwrap(),
        );
    }

    #[test]
    fn recommend_detects_frameworks_in_workspace_members() {
        // build_recommendation reads workspace members' package.json, so a
        // monorepo whose frameworks live in packages (not the root) surfaces them
        // in frameworks_present. Regression for the root-only detection gap.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"private":true,"devDependencies":{"typescript":"^5"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("pnpm-workspace.yaml"),
            "packages:\n  - 'apps/*'\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("apps/web")).unwrap();
        std::fs::write(
            root.join("apps/web/package.json"),
            r#"{"name":"@acme/web","dependencies":{"next":"^14","react":"^18"}}"#,
        )
        .unwrap();

        let present: Vec<String> =
            build_recommendation(root).to_json()["detected"]["frameworks_present"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
        assert!(
            present.contains(&"next".to_owned()) && present.contains(&"react".to_owned()),
            "frameworks in workspace members must surface in frameworks_present, got {present:?}"
        );
    }

    #[test]
    fn recommendation_human_report_points_to_json_for_full_detail() {
        let report = recommendation_human_report(&build_recommendation_from(&ts_lib_info(), &[]));
        assert!(
            report.contains("fallow recommend --format json"),
            "the human recommend summary must point to --format json for the full decision set"
        );
        // The concise human view still shows the zero-config stop.
        assert!(report.contains("Zero config is a valid stop"));
    }
}
