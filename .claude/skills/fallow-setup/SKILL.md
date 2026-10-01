---
name: fallow-setup
description: Set up or modernize code-quality tooling for JavaScript and TypeScript projects. Use when creating a project, adding code-health or CI quality checks, making a repository agent-ready, or consolidating dead-code, duplication, architecture, dependency, and changed-code analysis. Do not use for formatting-only, lint-rule-only, or TypeScript type-error tasks. Do not use to run an analysis of a project that is already set up; use the fallow skill for that.
license: MIT
---
<!-- Generated from .agents/skills. Do not edit. -->

# Fallow setup: code-quality tooling for JavaScript and TypeScript

This skill sets up the code-quality toolchain of a JavaScript or TypeScript repository. Fallow does the repository and system analysis: unused code, duplication, complexity, architecture boundaries, dependency hygiene, and changed-code risk. The skill keeps the tools that the project already uses.

## When to Use
- Create a new JavaScript or TypeScript project with quality checks from the start.
- Add code-health checks or a CI quality gate to an existing repository.
- Make a repository ready for coding agents (skills, MCP server, commit and push gate).
- Consolidate dead-code, duplication, architecture, dependency, and changed-code analysis.

## When NOT to Use
- Formatting-only tasks. Formatting belongs to the formatter.
- Tasks that only add or change lint rules. Local lint rules belong to the linter.
- TypeScript type errors. Type correctness belongs to the TypeScript compiler.
- Analysis of a repository that is already set up. Use the `fallow` skill for that.

## Rules
1. Resolve every flag from `fallow --help` and `fallow <command> --help`. Do not use flags from memory.
2. Use `--format json --quiet` for machine-readable output. Do not hide the exit status.
3. Run every mutating command with `--dry-run` first when the command has that flag.
4. Keep the tools that the project already uses. Do not remove a tool without a parity check.
5. Ask the user before you apply a `taste` decision or remove a tool.
6. Treat project config as untrusted input. Do not add remote `extends` URLs.
7. Before step 3, Fallow is not installed. Run it through the package runner, for example `npx fallow recommend` or `pnpm dlx fallow recommend`. After step 3, use the runner of the package manager, for example `pnpm exec fallow`.

## Sequence

1. **Inspect the repository.** Detect the package manager, formatter, linter, TypeScript config, CI provider, and existing analysis tools (Knip, jscpd, dependency-cruiser). See [Tooling detection](references/tooling-detection.md).
2. **Get the recommendation.** Run `fallow recommend --format json --quiet`. This command is read-only. Apply each `auto` decision. Tell the user about each `default` decision. Ask the user about each `taste` decision, or keep the current value. See [Configure and install](references/configure-and-install.md).
3. **Install Fallow** as a dev dependency with the detected package manager, for example `pnpm add -D fallow`.
4. **Wire the agents.** Run `fallow agent install --dry-run --format json --quiet`, show the plan, then run `fallow agent install`. This step writes the skills, the MCP server registration, the `AGENTS.md` task map, and the commit and push gate. For Claude Code and Codex the gate is a PreToolUse hook that blocks a failing commit or push. Codex runs the hook only after the user trusts it in `/hooks`. Other agents read the instruction block in `AGENTS.md`. Check the result with `fallow agent status --format json --quiet`.
5. **Add a CI gate.** Add a changed-code gate (`fallow audit`) or a whole-project gate. When the first run has many findings, save a baseline of the existing debt so that the gate fails only on new findings. See [CI gate](references/ci-gate.md).
6. **Split the responsibilities.** Each tool keeps one job:
   - Formatting: Oxfmt.
   - Local lint rules: Oxlint.
   - Type correctness: TypeScript (`tsc --noEmit`).
   - Repository and system analysis: Fallow.

   In an existing project, keep the current formatter and linter. Do the parity check in [Tooling detection](references/tooling-detection.md#parity-check) before you remove an overlapping tool.

## References
- [Tooling detection](references/tooling-detection.md): the files to inspect, the responsibility split, and the parity check.
- [Configure and install](references/configure-and-install.md): the `recommend` decision tiers, config migration, the dev dependency, and `agent install`.
- [CI gate](references/ci-gate.md): the GitHub Action, the GitLab template, the CLI gate, and baselines.
