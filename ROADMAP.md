# Fallow Roadmap

> This roadmap covers planned work and is reviewed periodically. For shipped capabilities, see the [releases](https://github.com/fallow-rs/fallow/releases) and [documentation](https://docs.fallow.tools).

This roadmap tracks planned work on Fallow: what is queued, what is being scoped, and where the project is headed.

---

## Next

Concrete work scoped to the next one or two minor releases.

### Richer MCP responses

In progress: agents already query fallow via MCP, but the responses lack context agents need to make confident removal decisions: re-export chains, who imports this symbol, recent churn, duplicate siblings. The work expands existing tool responses before adding new tools, and is partially landed.

### Coverage sidecar ergonomics

The coverage setup state machine works end to end, but the install handoff still depends on users trusting a download. Target: reproducible sidecar pinning, smoother framework recipe generation, clearer failure messages when the sidecar cannot attach.

### Post-fix formatter integration

`fallow fix` leaves Prettier, dprint, or Biome to clean up whitespace after removals. Invoke the project's configured formatter automatically when running in-place.

### Baseline-adoption ergonomics

Follow-ups to the `fallow.changedSince` setting shipped for issue #185. The setting works (Problems panel and sidebar scope to files changed since the configured ref) but a few UX polish items would close the loop for users adopting fallow on legacy codebases:

- **"Fallow: Set Baseline at HEAD" command** -- a palette command that runs `git tag fallow-baseline` and writes `fallow.changedSince` into `.vscode/settings.json` in one step, so users do not need to leave the editor or know the git tag command.
- **Filter-dropped status surfacing** -- when the LSP cannot resolve the configured ref (typo, shallow clone, missing tag), it currently falls back to full scope and logs a `WARNING` to the Fallow output channel. Surface that state in the status bar (e.g. `Fallow: 118 issues (since fallow-baseline: scope dropped)`) so users notice immediately rather than after the next "wait, why am I seeing all these issues again?" question.
- **Shallow-clone hint in CI templates** -- the runtime hint already explains the `fetch-depth: 0` fix; the GitHub Action template should default to a checkout depth that works with long-lived baseline tags, or document the requirement in the inline comments. The GitLab template ships `GIT_DEPTH: "0"` as a default since v2.54.0.

### Per-package `changedSince` overrides

Monorepos with packages on different release cadences want different baseline refs per package (e.g. `packages/web` tracks `main`, `packages/legacy` tracks `release/2024.10`). Today `fallow.changedSince` is workspace-wide. Extending this to per-package overrides requires config-schema work (a new `[overrides]` block keyed on workspace root, or `package.json` field), resolution semantics (which baseline wins for a file in package A imported from package B), and matching status-bar logic.

---

## Vision

Broader bets, still being scoped.

### Agent-driven cleanup loop

Safe removals (unused exports, enum members, dependencies) are already auto-fixable. The open question is the judgment calls: deleting files, consolidating duplicates, restructuring modules. The bet: structured MCP output plus the right review workflow lets an agent propose those changes, a human approves the PR, and fallow verifies nothing regressed.

### Health score calibration and adoption

Shipped today: `fallow health` provides a 0-100 score, an A-F letter grade,
badge output, saved vital-sign snapshots, and trend comparisons. Planned work
focuses on calibrating the formula against a broad real-world corpus, explaining
how its multiple signals contribute to each result, and helping teams adopt
baselines and thresholds that fit their project context. This direction improves
confidence and multi-signal explainability rather than adding another grade.

### Visualization

`fallow viz`: a self-contained interactive HTML report. Treemap with dead code highlighted, dependency graph, cycle visualization, duplication heatmaps. No server, opens in any browser. Scoping depends on which view actually unblocks a user workflow rather than just looking good in screenshots.

---

## Ongoing

Continuous work across releases.

- **Incremental analysis** -- finer-grained caching for faster watch mode and CI on large monorepos
- **Plugin ecosystem** -- more framework coverage, better external plugin authoring, community-contributed plugins
- **Health intelligence** -- structured fix suggestions, HTML report cards, richer regression diffing
- **Agent integration** -- Cursor integration, expanded MCP coverage, new editor surfaces beyond VS Code and Zed

---

## Known limitations

Acknowledged gaps. Fixes land opportunistically.

- **Syntactic analysis only** -- no TypeScript type information. Projects using `isolatedModules: true` (the modern default) are well-served; legacy tsc-only patterns may produce false positives.
- **Config parsing ceiling** -- AST-based extraction handles static configs. Computed values and conditionals are out of reach without JS eval.
- **Svelte export false negatives** -- props (`export let`) can't be distinguished from utility exports without Svelte compiler semantics.
- **NestJS/DI class members** -- abstract methods consumed via DI are not tracked. Use `unused_class_members = "off"` for DI-heavy projects.

---

[Open an issue](https://github.com/fallow-rs/fallow/issues) to request a feature or report a bug. PRs welcome: check the [contributing guide](CONTRIBUTING.md) and [issues labeled "good first issue"](https://github.com/fallow-rs/fallow/issues?q=label%3A%22good+first+issue%22).
