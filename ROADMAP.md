# Fallow Roadmap

> This roadmap covers planned work and is reviewed periodically. For shipped capabilities, see the [releases](https://github.com/fallow-rs/fallow/releases) and [documentation](https://fallow.tools/docs/).

---

## Next

Concrete work scoped to the next one or two minor releases.

### Richer MCP responses

The `inspect_target` tool already combines re-export chains, importers, duplicate siblings, and optional recent churn into one evidence bundle. The remaining work is to include the same context in broader MCP analysis responses that currently require a separate inspection call.

### Coverage sidecar ergonomics

Coverage setup works end to end. Installation still requires users to trust a download. Planned improvements include reproducible sidecar pinning, simpler framework recipe generation, and clearer failure messages when the sidecar cannot attach.

### Post-fix formatter integration

`fallow fix` leaves Prettier, dprint, or Biome to clean up whitespace after removals. Invoke the project's configured formatter automatically when running in-place.

---

## Vision

Broader bets, still being scoped.

### Agent-driven cleanup loop

Safe removals (unused exports, enum members, dependencies) are already auto-fixable. Deleting files, consolidating duplicates, and restructuring modules still need review. The proposal is to use structured MCP output so an agent can suggest these changes, a human can approve the PR, and fallow can check for regressions.

### Health score calibration and adoption

Shipped today: `fallow health` provides a 0-100 score, an A-F letter grade,
badge output, saved vital-sign snapshots, and trend comparisons. Planned work
focuses on calibration against a broad real-world corpus and
multi-signal explainability: showing how each signal contributes to the score
and grade.
Teams should be able to choose baselines and thresholds that fit their projects.

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

- **Syntactic by default** -- the fast Rust-native path does not require
  TypeScript. The optional `--type-aware` companion adds bounded checker
  evidence for exact symbol use, API leaks, targeted tests, and public type
  coupling. It does not replace compiler diagnostics or general typed linting.
- **Config parsing ceiling** -- AST-based extraction handles static configs. Computed values and conditionals are out of reach without JS eval.
- **Svelte export false negatives** -- props (`export let`) can't be distinguished from utility exports without Svelte compiler semantics.
- **NestJS/DI class members** -- abstract methods consumed via DI are not tracked. Use `unused_class_members = "off"` for DI-heavy projects.

---

[Open an issue](https://github.com/fallow-rs/fallow/issues) to request a feature or report a bug. PRs welcome: check the [contributing guide](CONTRIBUTING.md) and [issues labeled "good first issue"](https://github.com/fallow-rs/fallow/issues?q=label%3A%22good+first+issue%22).
