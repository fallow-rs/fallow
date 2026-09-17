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
pub const TOOL_GUIDE_NOTE: &str = "Long-form per-flag detail for one tool, moved out of its tools/list description. The wire description stays the routing summary; read this only for the tool you are about to call.";

const CHECK_HEALTH_SECTIONS: &[ToolGuideSection] = &[
    ToolGuideSection {
        topic: "css",
        summary: "What `css_analytics` contains and why it is opt-in.",
        detail: r"Set css=true to add a `css_analytics` section: specificity hotspots, `!important` density, over-complex selectors, deep nesting, design-token sprawl (distinct color/font-size/z-index counts), and unreferenced custom-property / `@keyframes` cleanup candidates (the structural CSS slop linters do not aggregate); opt-in because it parses every project stylesheet (standard CSS only, SCSS skipped).",
    },
    ToolGuideSection {
        topic: "complexity_breakdown",
        summary: "What a `contributions[]` entry names, and how JSX depth is carried.",
        detail: r"Set complexity_breakdown=true to add a `contributions[]` array to each complexity finding, breaking the cyclomatic and cognitive scores down per decision point (each entry names the construct: if, else-if, ternary, boolean operator, loop, case, catch, and on React/Preact components hook-density / prop-count, with its source line and weight) so you can explain WHY a function scored high and which specific lines to refactor. JSX depth is carried as descriptive `react_jsx_max_depth` context, not a contribution.",
    },
    ToolGuideSection {
        topic: "react_hook_profile",
        summary: "The React/Preact per-component hook breakdown carried on every React finding.",
        detail: r"React/Preact complexity findings also carry a `react_hook_profile` object (always present, no flag needed, omitted for non-React findings): a per-component hook breakdown (`state`/`effect`/`memo`/`callback`/`custom` counts) plus `max_effect_dep_arity` (the largest useEffect dependency-array arity over effects with a literal deps array). It refines the bare `react_hook_count` headline so you can spot effect-soup (many `effect`) and large effect dep-arrays (high `max_effect_dep_arity`) as the actionable triage signals; the breakdown covers component-scope hooks only, so it may sum to LESS than `react_hook_count` when a `use*` call sits in a plain helper.",
    },
    ToolGuideSection {
        topic: "vital_signs.render_fan_in",
        summary: "Render-fan-in concentration on React/Preact projects.",
        detail: r#"On React/Preact projects `vital_signs` also reports render-fan-in concentration (`p95_render_fan_in`, `render_fan_in_high_pct`, `max_render_fan_in`), the component-graph analogue of module fan-in: where module fan-in counts importing MODULES, render fan-in counts distinct render LOCATIONS of a component (a shared `<Button>` is rendered in far more places than it is imported), surfaced as descriptive blast-radius context (not a gate or finding). The headline `max_render_fan_in` is the highest DISTINCT-PARENTS count (the honest edit-ripple count); test / spec / story / fixture files are excluded. `vital_signs.top_render_fan_in` lists the highest-fan-in components sorted by distinct parents (each with `component` name, project-relative `path`, `distinct_parents` as the headline, and `render_sites` as secondary "incl. repeats" context) so you can see WHICH components are the blast-radius hotspots, not just the `max_render_fan_in` number."#,
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
        detail: r#"Synthetic `<template>` findings are NOT Angular-only: they fire on Angular `.html` and inline `@Component({ template: ... })` literals, Vue SFCs, Svelte components, and Astro components, each scored against its own control-flow vocabulary. Svelte components additionally emit each top-level `{#snippet name(...)}` block as its own `<snippet:NAME>` unit (an exact-match key for `health.thresholdOverrides[].functions`), scored with nesting rebased to zero, so in-file snippet extraction moves the score; snippets nested inside logic blocks or other snippets stay folded into the parent template unit. On `.svelte`, `.vue` and `.astro` files the `suppress-line` action uses `placement: "above-template-anchor-line"` with the markup comment `<!-- fallow-ignore-next-line complexity -->`, which must sit on the line immediately preceding the reported line (the template unit is anchored at its first contributing construct, not at the top of the file). Synthetic template-family units (`<template>` and `<snippet:NAME>`) are NOT scored on the CRAP dimension: a template carries no direct test coverage, so template findings never include `crap`, `coverage_pct`, `coverage_tier`, `coverage_source`, or `inherited_from` and gate on the cyclomatic and cognitive dimensions only. A `maxCrap` override scoped to a template unit reports a matched crap-dimension row explaining the entry can be removed. Branching outside every function (a top-level `if` ladder, a module-scope `??` / `||` default, an `?.` access on a config object) is extracted as a synthetic `<module>` unit, one per file that actually branches at module scope. It is aggregate-only: it feeds `vital_signs` (average, critical share, p90 cyclomatic), the per-file complexity totals and density, and the review brief's branching conservation, and it never appears as a finding, in `large_functions`, on the CRAP dimension, or as a `health.thresholdOverrides[].functions` key. Do not expect a `<module>` entry in `findings`; read it in the aggregates. Svelte await-block entries use explicit `await`, `then`, and `catch` kinds."#,
    },
    ToolGuideSection {
        topic: "component_rollup",
        summary: "The Angular class-plus-template rollup finding.",
        detail: r#"Angular components whose class AND template both contribute to complexity also emit a synthetic `<component>` rollup finding anchored at the worst class method's `(line, col)`. The rollup's `cyclomatic` is `worst_class_method.cyclomatic + template.cyclomatic` (the same worst-by-cyclomatic method drives both metrics; cognitive is `worst.cognitive + template.cognitive`). The `component_rollup` payload carries the pre-summation breakdown: `class_worst_function` (method name), `class_cyclomatic` / `class_cognitive` (per-method numbers), `template_path` / `template_cyclomatic` / `template_cognitive`, plus a `component` identifier derived from the .ts owner's file stem. The rollup's `suppress-line` action uses `placement: "above-component-worst-method"`: a `// fallow-ignore-next-line complexity` placed above the worst class method hides BOTH the per-function finding AND the rollup, so agents do not need to emit two suppression edits. Per-function and per-`<template>` entries stay alongside the rollup; ranking and `--targets` use the rollup so a template-heavy component surfaces as one unit rather than scattered medium findings."#,
    },
    ToolGuideSection {
        topic: "threshold_overrides",
        summary: "How to read one `threshold_overrides` state row.",
        detail: r"Each state row carries a `dimension` (`complexity` or `crap`): one configured override produces one row per dimension it participates in, so group on `override_index` to count configured overrides rather than counting rows. A row's `outstanding[]` names every dimension on which the matched unit STILL produces a finding after the override applied, whether the entry leaves that ceiling unconfigured (the override reads `active` next to a surviving finding) or raises it to a value the unit still exceeds (the row reads `insufficient`).",
    },
    ToolGuideSection {
        topic: "gate_outcomes",
        summary: "How a gated run reports its verdict when the exit code cannot.",
        detail: r"A run that armed a gate carries `gate_outcomes` at the envelope root, keyed by gate name. `min_score` produces a `health-min-score` entry and `min_severity` a `health-min-severity` one; `health-findings` reports `skipped` when `min_score` made complexity findings informational. Each entry carries `status` (`pass`, `warn`, `fail`, `skipped`), `enforced` (whether a `fail` from it makes the CLI exit non-zero), and `observed` / `threshold` where the gate compared a number. A gate failed the run when `status` is `fail` AND `enforced` is true; `enforced` alone is true on every armed gate including the ones that passed. A baselined run carries `baseline_staleness` alongside it: read `gate_trips` for the `--fail-on-stale-baseline` rule and `change_scoped` before dividing `matched_entries` by `baseline_entries`. The MCP server converts the CLI's exit 1 into a successful result so the findings still reach you, so these two objects are the verdict; every entry that reported `fail` or `warn`, and a baseline that matched less than it was saved with, is also restated as a plain sentence in the result's root `warnings` array.",
    },
];

const GET_CLOUD_RUNTIME_CONTEXT_SECTIONS: &[ToolGuideSection] = &[
    ToolGuideSection {
        topic: "FALLOW_API_KEY",
        summary: "Where the key comes from and what an absent one returns.",
        detail: r#"The key is read from `FALLOW_API_KEY` in the environment of the MCP server process, so it is configured once where the server is launched and never travels in a tool call. A key set to whitespace counts as absent. A call made without one is refused before any subprocess starts, with `isError`, and a body carrying `error: true`, `exit_code: 2`, `code: "cloud_api_key_missing"`, and the same remediation sentence the CLI prints for `coverage analyze --cloud`. Restarting the server is what picks up a newly exported variable; nothing rereads it per call. The refusal is not a fallback signal to retry with different parameters: without a key this tool can answer nothing, and the local `check_runtime_coverage` with a `coverage` path is the alternative."#,
    },
    ToolGuideSection {
        topic: "root",
        summary: "Why the checkout matters as much as the repository name.",
        detail: r"The cloud returns functions by file path and name, and those are joined against the static analysis of the project at `root` before anything is reported. A cloud function that no longer matches a definition in the checkout is dropped from the merge rather than reported, and counted in a `cloud_functions_unmatched` warning, so pointing `root` at an unrelated project, or at a checkout many commits away from what production runs, quietly empties the findings instead of failing. Pin `commit_sha` to the deployed revision, or check out that revision, when the answer has to line up with a specific deployment.",
    },
    ToolGuideSection {
        topic: "period_days",
        summary: "What the observation window changes, and its bounds.",
        detail: r#"`period_days` selects how far back the cloud aggregates runtime observations, from 1 to 90, defaulting to 30. A value outside that range is refused locally with `code: "cloud_period_out_of_range"` rather than spending a round trip. The window is the denominator of the whole answer: a short window makes rarely-exercised code look never-called, which is the failure mode this tool has to be read carefully for, while a long window blends several deployments together. `summary.deployments_seen` and `summary.last_received_at` say what the window actually contained."#,
    },
    ToolGuideSection {
        topic: "runtime_coverage.warnings",
        summary: "The codes that explain a thin or surprising answer.",
        detail: r"`no_runtime_data` means the cloud holds no observations for the selection at all, which is a configuration answer (wrong repository, wrong environment, no beacon reporting) and not evidence that the code is cold. `cloud_functions_unmatched` counts functions the cloud reported that the checkout no longer has, and is the signal that the two sides are on different revisions. Codes prefixed `cloud_warning_` are passed through from the cloud unchanged. Read these before acting on an empty or near-empty `findings` array.",
    },
];

/// Every tool with a long-form guide, in catalogue order.
pub const TOOL_GUIDES: &[ToolGuide] = &[
    ToolGuide {
        tool: "check_health",
        sections: CHECK_HEALTH_SECTIONS,
    },
    ToolGuide {
        tool: "get_cloud_runtime_context",
        sections: GET_CLOUD_RUNTIME_CONTEXT_SECTIONS,
    },
];

/// The guide for one wire tool name, if it has one.
#[must_use]
pub fn tool_guide(name: &str) -> Option<&'static ToolGuide> {
    TOOL_GUIDES.iter().find(|guide| guide.tool == name)
}
