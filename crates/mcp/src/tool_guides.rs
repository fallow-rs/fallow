//! Long-form per-tool detail, served as the `fallow://tools/{name}` resource
//! template.
//!
//! The `tools/list` wire description is a routing summary: what a tool
//! returns, when to reach for it, and how it differs from its siblings. Every
//! agent pays for it on every session, so per-flag payload shapes, unit
//! vocabularies, and suppression placements live here instead, where an agent
//! reads them once and only for the tool it actually called.
//!
//! This is NOT the `fallow://tools` manifest resource: that one is the terse
//! catalogue (one line per tool). Two drift tests in `server/tests` hold that
//! line: one keeps guide prose out of the catalogue, and
//! `tool_descriptions::catalogue_lines_stay_shorter_than_the_wire_description`
//! keeps every catalogue line shorter than the tool's own `tools/list` text.
//! Long prose belongs in this template only.

/// One topic of a tool's long-form guide, keyed by the parameter or output
/// section it explains.
pub struct ToolGuideSection {
    /// Parameter or output key this section explains.
    pub topic: &'static str,
    /// One line naming what the detail below answers.
    pub summary: &'static str,
    /// The prose moved out of the wire description, verbatim.
    pub detail: &'static str,
}

/// The long-form guide for one tool.
pub struct ToolGuide {
    /// Wire tool name, matching a [`fallow_types::mcp_manifest::MCP_TOOLS`] entry.
    pub tool: &'static str,
    /// Ordered topics.
    pub sections: &'static [ToolGuideSection],
}

/// Shared note explaining what a guide is and is not, so a cached copy is
/// self-describing.
pub const TOOL_GUIDE_NOTE: &str = "This guide explains parameters and output for one tool. Read only the guide for the tool you are about to call.";

const CHECK_HEALTH_SECTIONS: &[ToolGuideSection] = &[
    ToolGuideSection {
        topic: "css",
        summary: "What `css_analytics` contains and why it is opt-in.",
        detail: r"Set css=true to add a `css_analytics` section. It reports specificity hotspots, `!important` density, over-complex selectors, deep nesting, and design-token sprawl through distinct color, font-size, and z-index counts. It also reports unreferenced custom-property and `@keyframes` cleanup candidates.

This analysis is opt-in because it parses every project stylesheet. Standard CSS is parsed structurally. Sass/Less sources are scanned only where fallow can stay conservative without expanding preprocessor semantics.",
    },
    ToolGuideSection {
        topic: "complexity_breakdown",
        summary: "What a `contributions[]` entry names, and how JSX depth is carried.",
        detail: r"Set complexity_breakdown=true to add a `contributions[]` array to each complexity finding. The array breaks down cyclomatic and cognitive scores per decision point. Each entry names the construct (if, else-if, ternary, boolean operator, loop, case, or catch), its source line, and its weight. React/Preact component entries also include hook-density and prop-count contributions. Use these entries to identify why a function scored high and which lines to refactor.

JSX depth is descriptive `react_jsx_max_depth` context, outside the contribution array.",
    },
    ToolGuideSection {
        topic: "react_hook_profile",
        summary: "When the React/Preact per-component hook breakdown is included.",
        detail: r"React/Preact complexity findings carry a `react_hook_profile` object only when component-scope hooks were attributed. It needs no extra flag and is omitted when no such hooks were attributed, including non-React findings. The per-component breakdown includes `state`, `effect`, `memo`, `callback`, and `custom` counts. It also reports `max_effect_dep_arity`, the largest useEffect dependency-array arity among effects with a literal deps array.

The breakdown refines the `react_hook_count` headline. A high `effect` count identifies components with many effects; a high `max_effect_dep_arity` identifies large effect dependency arrays. The breakdown counts component-scope hooks only. Its total may be lower than `react_hook_count` when a `use*` call sits in a plain helper.",
    },
    ToolGuideSection {
        topic: "vital_signs.render_fan_in",
        summary: "Render-fan-in concentration on React/Preact projects.",
        detail: r"On React/Preact projects, `vital_signs` reports render-fan-in concentration through `p95_render_fan_in`, `render_fan_in_high_pct`, and `max_render_fan_in`. Module fan-in counts importing modules. Render fan-in counts distinct rendering parents, keyed by source file and parent component. A module-level render also counts, keyed to its source file with no parent component. A shared `<Button>` can have more rendering parents than importing modules. These metrics are descriptive context and create no gate or finding.

The headline `max_render_fan_in` is the highest distinct-parent count. Test, spec, story, and fixture files are excluded. `vital_signs.top_render_fan_in` lists the components with the highest fan-in, sorted by distinct parents. Each entry carries the `component` name, project-relative `path`, and `distinct_parents` as the headline. Its `render_sites` count includes repeated renders and provides secondary context.",
    },
    ToolGuideSection {
        topic: "churn_file",
        summary: "Importing VCS history for the churn-backed signals.",
        detail: r"Set churn_file to a `fallow-churn/v1` JSON path to power the churn-backed signals (hotspots, ownership, and refactoring targets) from imported VCS history instead of git, so they work on projects with no git repository (Yandex Arc, Mercurial, Perforce); a small wrapper translates the VCS log into the contract, and the `since` window then only labels output since the file is authoritative.",
    },
    ToolGuideSection {
        topic: "coverage_source",
        summary: "How a CRAP finding reports where its coverage came from.",
        detail: r"CRAP findings carry a `coverage_source` discriminator (`istanbul`, `estimated`, or `estimated_component_inherited`); `summary.coverage_source_consistency` and grouped `coverage_source_consistency` report whether emitted CRAP finding sources are uniform or mixed.",
    },
    ToolGuideSection {
        topic: "synthetic units",
        summary: "The `<template>`, `<snippet:NAME>` and `<module>` units scored alongside real functions.",
        detail: r#"Synthetic `<template>` findings apply to Angular `.html` and inline `@Component({ template: ... })` literals, Vue SFCs, Svelte components, and Astro components. Each is scored against its own control-flow vocabulary.

Svelte components also emit each top-level `{#snippet name(...)}` block as its own `<snippet:NAME>` unit. This is an exact-match key for `health.thresholdOverrides[].functions`. Snippet nesting is rebased to zero, so extracting an in-file snippet changes the score. Snippets nested inside logic blocks or other snippets stay folded into the parent template unit.

On `.svelte`, `.vue`, and `.astro` files, the `suppress-line` action uses `placement: "above-template-anchor-line"` with the markup comment `<!-- fallow-ignore-next-line complexity -->`. The comment must sit on the line immediately preceding the reported line. The template unit is anchored at its first contributing construct, rather than the top of the file.

Synthetic template-family units (`<template>` and `<snippet:NAME>`) are not scored on the CRAP dimension because a template carries no direct test coverage. Their findings never include `crap`, `coverage_pct`, `coverage_tier`, `coverage_source`, or `inherited_from`. They gate on cyclomatic and cognitive dimensions only. A `maxCrap` override scoped to a template unit reports a matched crap-dimension row explaining that the entry can be removed.

Branching outside every function is extracted as a synthetic `<module>` unit, one per file that branches at module scope. Examples include a top-level `if` ladder, a module-scope `??` or `||` default, and an `?.` access on a config object. This unit is aggregate-only. It feeds `vital_signs` (average, critical share, and p90 cyclomatic), per-file complexity totals and density, and the review brief's branching conservation. It never appears as a finding, in `large_functions`, on the CRAP dimension, or as a `health.thresholdOverrides[].functions` key. Read `<module>` in the aggregates; it has no entry in `findings`.

Svelte await-block entries use explicit `await`, `then`, and `catch` kinds."#,
    },
    ToolGuideSection {
        topic: "component_rollup",
        summary: "The Angular class-plus-template rollup finding.",
        detail: r#"Angular components whose class and template both contribute to complexity also emit a synthetic `<component>` rollup finding. It is anchored at the worst class method's `(line, col)`. The rollup's `cyclomatic` is `worst_class_method.cyclomatic + template.cyclomatic`. The same worst-by-cyclomatic method drives both metrics; cognitive is `worst.cognitive + template.cognitive`.

The `component_rollup` payload reports the breakdown before summation. It carries `class_worst_function` (method name), `class_cyclomatic` and `class_cognitive` (per-method numbers), and `template_path`, `template_cyclomatic`, and `template_cognitive`. Its `component` identifier comes from the .ts owner's file stem.

The rollup's `suppress-line` action uses `placement: "above-component-worst-method"`. A `// fallow-ignore-next-line complexity` placed above the worst class method hides both the per-function finding and the rollup. One suppression edit covers both findings. Per-function and per-`<template>` entries stay alongside the rollup. Ranking and `--targets` use the rollup so a template-heavy component appears as one unit instead of scattered medium findings."#,
    },
    ToolGuideSection {
        topic: "threshold_overrides",
        summary: "How to read one `threshold_overrides` state row.",
        detail: r"Each state row carries a `dimension` (`complexity` or `crap`). One configured override produces one row per dimension it participates in. Group on `override_index` to count configured overrides rather than rows.

A row's `outstanding[]` names every dimension on which the matched unit still produces a finding after the override applies. An override can read `active` alongside a surviving finding when it leaves that dimension's ceiling unconfigured. A row reads `insufficient` when the override raises the ceiling to a value the unit still exceeds.",
    },
    ToolGuideSection {
        topic: "gate_outcomes",
        summary: "How a gated run reports its verdict when the exit code cannot.",
        detail: r"A CLI-backed result carries `gate_outcomes` at the envelope root, keyed by gate name. It includes the default exit rule, `health-findings`, even when no gate was armed, unless `min_severity` replaces that rule. `min_score` produces a `health-min-score` entry. `min_severity` produces a `health-min-severity` entry. When `min_score` is set without `min_severity`, `health-findings` reports `skipped` and is unenforced; complexity findings are informational.

Each entry carries `status` (`pass`, `warn`, `fail`, or `skipped`) and `enforced` (whether a `fail` from it makes the CLI exit non-zero). It also carries `observed` and `threshold` where the gate compared a number. A gate failed the run only when `status` is `fail` and `enforced` is true. A passed gate can still be enforced. Read both `status` and `enforced`.

A baselined run also carries `baseline_staleness`. Read `gate_trips` for the `--fail-on-stale-baseline` rule. Check `change_scoped` before dividing `matched_entries` by `baseline_entries`. Read `scope_reasons` for the channels that narrowed the run.

The MCP server converts the CLI's exit 1 into a successful result so the findings reach you. Read `gate_outcomes` and `baseline_staleness` for the verdict. Every entry that reported `fail` or `warn`, and a baseline that matched less than it was saved with, is also restated as a plain sentence in the result's root `warnings` array.",
    },
];

const ANALYZE_SECTIONS: &[ToolGuideSection] = &[
    ToolGuideSection {
        topic: "absent_component_props",
        summary: "Optional props that inspected callers do not supply.",
        detail: r#"Select issue_types: ["absent-component-props"] to enable this default-off rule. Each candidate carries the declaration, framework, default presence and inspected caller locations. Review defaults and API intent manually; static evidence does not prove runtime unreachability. Its actions are not auto-fixable. Configured warning or error severity still controls the normal issue gates."#,
    },
    ToolGuideSection {
        topic: "boundary_violations",
        summary: "The alias that limits a run to architecture boundary violations.",
        detail: r#"Set boundary_violations=true to check only architecture boundary violations. It is a convenience alias for issue_types: ["boundary-violations"]. The response is the same structured JSON as a full run, with all issues found of that one type."#,
    },
    ToolGuideSection {
        topic: "group_by",
        summary: "What each grouping mode keys on.",
        detail: r#"Set group_by to "owner", "directory", "package", or "section" to group results. `owner` groups by the CODEOWNERS owner of each finding file, `directory` by its first directory, and `package` by its workspace package. The `section` mode reads GitLab CODEOWNERS `[Section]` headers and emits `owners` metadata per group. A cycle finding goes into one group: a circular dependency or re-export cycle by its first file, and a package cycle by the file of its first example import. Each finding keeps its `finding_id` inside its group, so an id compares the same way in a grouped and an ungrouped run."#,
    },
    ToolGuideSection {
        topic: "next_steps",
        summary: "How to dispatch the follow-up commands a response suggests.",
        detail: r"Responses also include a top-level `next_steps[]` array of read-only follow-up commands (`{id, command, reason}`) computed from the findings. The stable `id` (e.g. `trace-unused-export`, `trace-clone`, `complexity-breakdown`) maps to a sibling tool or `code_execute` host call (`traceExport`, `traceClone`, `checkHealth({complexity_breakdown:true})`), so dispatch on `id` rather than running the CLI `command` string verbatim.",
    },
];

const GET_CLOUD_RUNTIME_CONTEXT_SECTIONS: &[ToolGuideSection] = &[
    ToolGuideSection {
        topic: "FALLOW_API_KEY",
        summary: "Where the key comes from and what an absent one returns.",
        detail: r#"The key is read from `FALLOW_API_KEY` in the MCP server process environment. Configure it where the server is launched; it never travels in a tool call. A whitespace-only key counts as absent. Restart the server after exporting a new value. The server does not inherit later changes to its launch environment.

A call without a key is refused before any subprocess starts. It returns `isError` and a body carrying `error: true`, `exit_code: 2`, `code: "cloud_api_key_missing"`, and the same remediation sentence the CLI prints for `coverage analyze --cloud`. This refusal does not justify retrying with different parameters: the tool cannot answer without a key. Use the local `check_runtime_coverage` with a `coverage` path as the alternative."#,
    },
    ToolGuideSection {
        topic: "root",
        summary: "Why the checkout matters as much as the repository name.",
        detail: r"The cloud returns functions by file path and name. Before reporting them, the tool joins them against static analysis of the project at `root`. A cloud function that no longer matches a definition in the checkout is dropped from the merge and counted in a `cloud_functions_unmatched` warning.

Pointing `root` at an unrelated project or a checkout many commits away from production can leave the findings empty without failing. Pin `commit_sha` to the deployed revision, or check out that revision, when the answer must align with a specific deployment.",
    },
    ToolGuideSection {
        topic: "period_days",
        summary: "What the observation window changes, and its bounds.",
        detail: r#"`period_days` selects how far back the cloud aggregates runtime observations, from 1 to 90, defaulting to 30. A value outside that range is refused locally with `code: "cloud_period_out_of_range"` before a network round trip.

A short window can make rarely exercised code appear never called. Do not treat a lack of observations as proof that code is unused. A long window blends several deployments together. `summary.deployments_seen` and `summary.last_received_at` report what the window contained."#,
    },
    ToolGuideSection {
        topic: "runtime_coverage.warnings",
        summary: "The codes that explain a thin or surprising answer.",
        detail: r"`no_runtime_data` means the cloud holds no observations for the selection at all, which is a configuration answer (wrong repository, wrong environment, no beacon reporting) and not evidence that the code is cold. `cloud_functions_unmatched` counts functions the cloud reported that the checkout no longer has, and is the signal that the two sides are on different revisions. Codes prefixed `cloud_warning_` are passed through from the cloud unchanged. Read these before acting on an empty or near-empty `findings` array.",
    },
];

const GET_CLOUD_REVIEW_PACKET_SECTIONS: &[ToolGuideSection] = &[
    ToolGuideSection {
        topic: "scope",
        summary: "What the call sends, and the default when it sends nothing.",
        detail: r"`files` are repo-relative paths and `functions` are `{file, name, line?}` targets, at most 1000 of each. With neither, the tool sends the source files changed against `base` in the checkout at `root`. The base resolves like `audit`: `base`, then `FALLOW_AUDIT_BASE`, then the merge-base with the upstream or the remote default branch. A checkout with no changed source files is refused with `exit_code: 2` before any network call.",
    },
    ToolGuideSection {
        topic: "tracking states",
        summary: "How to read `tracking_state` against `period_tracking_state`.",
        detail: r"`tracking_state` covers only the current deployment. A service that deploys several times a day can hold only hours of evidence for it, which `evidence_window.observed_hours` states. `period_tracking_state` covers every deployment of the period: `called` when the function ran in any of them. Boot code that a new deployment has not run yet is `never_called` now and `called` over the period, so never treat it as dead. `repo_path` is `file_path` without the proven runtime prefix; it is null when the cloud proved no prefix.",
    },
];

/// The literal-search fields shared by `trace_export` and `impact_closure`.
const SEARCH_VISIBILITY_DETAIL: &str = r"`search_visibility` says whether a whole-word text search for the export name (`grep -w`) finds the consumer. `found`: the consumer source contains the name and uses it under that name. `renamed`: the source contains the name, but the uses go by another local name. `missed`: the source does not contain the name. `reason` names the cause: `import-alias` (`import { foo as bar }`), `default-import` (a default import under another name), `re-export-alias` (a barrel renames the export), `namespace-use` (`import * as ns` with no `ns.foo`), or `dynamic-import`. A barrel that only forwards the export (`export * from`, `export { foo } from`) and has no import of it is `found` with `reason` `pass-through`: it needs no edit when the export changes, and the files that import through it are checked on their own. `search_as` is the name to search for in that consumer instead. The check reads the consumer source when the tool runs. A match in a comment or a string counts as found, because a text search finds it too. The fields are absent when no check applies, for example when the consumer source cannot be read. A class member trace (`FILE:MEMBER`) carries the same fields on `owner_direct_references[]`. There the check uses the owner export name, because a search for the class name finds the users of the class.";

const TRACE_EXPORT_SECTIONS: &[ToolGuideSection] = &[ToolGuideSection {
    topic: "direct_references.search_visibility",
    summary: "Which consumers a text search for the export name does not find.",
    detail: SEARCH_VISIBILITY_DETAIL,
}];

const IMPACT_CLOSURE_SECTIONS: &[ToolGuideSection] = &[ToolGuideSection {
    topic: "coordination_gap.search_visibility",
    summary: "Which gap consumers a text search for the symbol names does not find.",
    detail: r"Each coordination gap carries `search_visibility`, `reason`, and `search_as` for the consumed symbols. When a consumer uses more than one symbol, the worst level wins: `missed`, then `renamed`, then `found`. The levels and reasons are the same as for `trace_export`: read fallow://tools/trace_export for their meaning.",
}];

const AUDIT_SECTIONS: &[ToolGuideSection] = &[
    ToolGuideSection {
        topic: "diff_risk",
        summary: "The rules that report facts about the change itself.",
        detail: r"The top-level `diff_risk` object holds a `findings` array. The rules compare the base text and the head text of each changed file, so every finding is introduced by the change. The object is absent when the change has no diff-risk finding.

- `weakened-tests`: the change weakens tests. `kind` tells how: `skip-only` (a new skip or focus marker such as `it.skip` or `it.only`), `removed-tests` (fewer test cases across the changed test files), `assertion-drop` (fewer assertions in a test file), or `coverage-threshold` (a less strict coverage or quality threshold).
- `added-suppression`: the change adds a lint, type-check, coverage, or Fallow suppression comment.
- `test-only-branch`: the change adds a test-environment branch, such as a `NODE_ENV` check, to non-test source code.
- `diverged-clone-sibling`: the change edits one copy of duplicated code, and its unchanged copies no longer match. `related` names the unchanged copies. This rule needs the base pass.
- `added-dependency`: the change adds a dependency to a `package.json`. This rule is off by default.

Each finding carries `rule`, `path`, an optional `line`, `message`, `evidence`, `severity`, and `actions`. An `error` finding fails the verdict, and a `warn` finding turns `pass` into `warn`. `attribution.diff_risk_introduced` counts the findings. The `rules` config sets the severity of each rule, and the rule name is also the suppression token. The actions only suppress (`suppress-line`, `suppress-file`, or `suppress-in-config`); none is auto-fixable. Review the change before you suppress a finding.",
    },
    ToolGuideSection {
        topic: "styling",
        summary: "What the default styling pass adds, and how css_deep changes it.",
        detail: r"Styling analytics are on by default in audit. CSS and CSS-in-JS evidence can add `styling_findings`, `css_analytics`, and `styling_health` inside the nested `complexity` block. Set css_deep=false to skip project-wide styling reachability and keep the local styling checks. Set css_deep=true to turn it on again when the config turns it off. Styling actions are report-only: verify each one before you edit.",
    },
    ToolGuideSection {
        topic: "coverage",
        summary: "How to feed Istanbul coverage into CRAP scoring.",
        detail: r"Set coverage to an Istanbul `coverage-final.json` path for accurate CRAP scoring in the health sub-analysis. Set coverage_root to an absolute coverage-data path prefix when the paths in the file need rebasing. CRAP findings in the `complexity` block carry `coverage_source`, and `summary.coverage_source_consistency` says whether the emitted sources are uniform or mixed.",
    },
    ToolGuideSection {
        topic: "complexity",
        summary: "The synthetic units and overrides in the audit `complexity` block.",
        detail: r#"Complexity findings include Angular `<component>` rollups that fold the worst class method and the template into one ranked unit. Synthetic `<template>` findings cover Angular, Vue, Svelte, and Astro markup. Top-level Svelte `{#snippet}` blocks are scored as their own `<snippet:NAME>` units. Template-family units are not scored on the CRAP dimension. Module-scope branching goes into a synthetic `<module>` unit that feeds the aggregate metrics and the branching totals, but never becomes a finding. On `.svelte`, `.vue`, and `.astro` files, a template-family finding suppresses with `<!-- fallow-ignore-next-line complexity -->` at `placement: "above-template-anchor-line"`.

When the config sets `health.thresholdOverrides`, audit complexity findings use the resolved local thresholds, and the `complexity` block includes `threshold_overrides` state. Read the fallow://tools/check_health guide for the `component_rollup` payload and the `threshold_overrides` rows."#,
    },
    ToolGuideSection {
        topic: "group_by",
        summary: "How to partition the audit results.",
        detail: r#"Set group_by to "owner" (CODEOWNERS), "directory", "package" (workspace), or "section" to partition the results. The `section` mode reads GitLab CODEOWNERS `[Section]` headers and adds `owners` metadata to each group."#,
    },
    ToolGuideSection {
        topic: "baselines",
        summary: "How saved baselines keep old issues out of the verdict.",
        detail: r"Set dead_code_baseline, health_baseline, and dupes_baseline to the baseline files that `fallow dead-code|health|dupes --save-baseline` wrote. Pre-existing issues on touched files then do not decide the verdict: only issues that are not in the matching baseline count.

Set include_entry_exports=true to also report unused exports in entry files. This finds typos in framework exports, such as `meatdata` for `metadata`. The flag ORs with the `includeEntryExports` config value. explain_skipped only changes the human-format note about skipped default ignores; MCP JSON responses do not change.",
    },
    ToolGuideSection {
        topic: "runtime_coverage",
        summary: "How to fold runtime coverage into the same audit call.",
        detail: r"Set runtime_coverage to a V8 coverage directory, a V8 JSON file, or an Istanbul `coverage-final.json` to add runtime-coverage findings to the audit. One call then gives the `hot-path-touched` verdict next to dead code and complexity. A single local capture is free; continuous or multi-capture monitoring needs an active license. The verdict is informational and does not change the exit code. Set min_invocations_hot=N to tune the hot-path threshold (default 100).

For PR review, `runtime_coverage.verdict` puts `hot-path-touched` before `cold-code-detected`. The full set of signals, with no priority, is in `runtime_coverage.signals[]`.",
    },
    ToolGuideSection {
        topic: "FALLOW_DIFF_FILE",
        summary: "How a diff file and the manifest scope narrow the findings.",
        detail: r"When `FALLOW_DIFF_FILE` (the path to a unified diff) is set in the environment of the agent process, every finding (dead code, complexity, duplication, boundary, and runtime-coverage hot paths) is narrowed to source lines inside an added hunk. Project-level findings in scope (unused dependencies, catalog entries, dependency overrides) skip the line filter, because they anchor at fixed `package.json` or `pnpm-workspace.yaml` lines that a PR rarely touches. When `FALLOW_DIFF_FILE` and `FALLOW_CHANGED_SINCE` are both set, the diff controls the line filter, and `--changed-since` still scopes file discovery.

Dependency-level findings (unused, misplaced, type-only, or test-only dependencies, overrides, and catalog entries) are in audit scope only when the changed set holds the package manifest that declares them, workspace manifests included. Unlisted dependencies follow imports. Use `analyze` to see every dependency finding.",
    },
];

/// Every tool with a long-form guide, in catalogue order.
pub const TOOL_GUIDES: &[ToolGuide] = &[
    ToolGuide {
        tool: "analyze",
        sections: ANALYZE_SECTIONS,
    },
    ToolGuide {
        tool: "check_health",
        sections: CHECK_HEALTH_SECTIONS,
    },
    ToolGuide {
        tool: "audit",
        sections: AUDIT_SECTIONS,
    },
    ToolGuide {
        tool: "get_cloud_runtime_context",
        sections: GET_CLOUD_RUNTIME_CONTEXT_SECTIONS,
    },
    ToolGuide {
        tool: "get_cloud_review_packet",
        sections: GET_CLOUD_REVIEW_PACKET_SECTIONS,
    },
    ToolGuide {
        tool: "trace_export",
        sections: TRACE_EXPORT_SECTIONS,
    },
    ToolGuide {
        tool: "impact_closure",
        sections: IMPACT_CLOSURE_SECTIONS,
    },
];

/// The guide for one wire tool name, if it has one.
#[must_use]
pub fn tool_guide(name: &str) -> Option<&'static ToolGuide> {
    TOOL_GUIDES.iter().find(|guide| guide.tool == name)
}
