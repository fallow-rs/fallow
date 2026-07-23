# Maintainer documentation

This is the entry point for durable knowledge in the Fallow repository. Use the
[task context map](development/task-context-map.md) to load only what a task
needs.

Public user documentation is authored in
[`fallow-docs`](https://github.com/fallow-rs/docs). The files below are
for contributors and maintainers of the open-source codebase.

## Development foundations

- [Development documentation](development/README.md): routing for shared
  architecture and workflow references.
- [Knowledge architecture](development/knowledge-architecture.md): ownership,
  trust boundaries, synchronization, and promotion policy.
- [AI tooling](development/ai-tooling.md): canonical skills and generated
  adapters for Codex and Claude.
- [Task context map](development/task-context-map.md): ordered reads by task.
- [Repository map](development/repo-map.md): crates, pipeline, and subsystem
  hotspots.
- [Quality gates](development/quality-gates.md): local verification and hook
  parity.
- [Review routing](development/review-routing.md): review lenses by changed
  path.

## Architecture and extension points

- [Architecture invariants](architecture-invariants.md): crate boundaries,
  dependency direction, and ownership rules.
- [Analyzer authoring](analyzer-authoring.md): adding and registering analyzers.
- [Plugin authoring](plugin-authoring.md): built-in plugin conventions.
- [Core migration](fallow-core-migration.md): migration to the engine and API
  layers.
- [Backwards compatibility](backwards-compatibility.md): stable CLI, config, and
  output contracts.

## Internal implementation references

- [Implementation references](reference/README.md): subsystem-specific durable
  detail extracted from runtime rules.
- [CLI internals](reference/cli-internals.md): command plumbing, reports, fixes,
  and format-specific invariants.
- [Detection internals](reference/detection-internals.md): extraction, graph,
  and analysis behavior.
- [Extraction internals](reference/extract-internals.md): parser dispatch,
  visitors, component files, and cache-sensitive extraction.
- [LSP internals](reference/lsp-internals.md): diagnostics, code actions, and
  protocol-specific constraints.
- [MCP internals](reference/mcp-internals.md): tool contracts, CLI fallbacks,
  and subprocess behavior.
- [Plugin internals](reference/plugin-internals.md): framework plugin discovery
  and configuration details.
- [VS Code internals](reference/vscode-internals.md): extension architecture,
  binary lifecycle, and editor integration details.

## Verification and operations

- [Benchmarking](benchmarking.md): performance measurement and benchmark
  maintenance.
- [Public config corpus](public-config-corpus.md): safe public configuration
  research.
- [Security agent verification](security-agent-verification.md): verification
  of syntactic security candidates.
- [Fallow compliance](fallow-compliance.md): compliance workflow.
- [Environment variables](environment-variables.md): supported environment
  controls.
- [Telemetry](telemetry.md): opt-in telemetry behavior and privacy.

## Styling validation

- [Styling corpus smoke](styling-corpus-smoke.md): corpus-wide styling checks.
- [Styling PR smoke](styling-pr-smoke.md): pull-request validation.
- [Styling release matrix](styling-release-matrix.md): release coverage across
  styling surfaces.

## Release context

- [Fallow v3 release notes draft](v3-release-notes.md): maintained release
  preparation context. Remove it from this index when it is retired.

## Placement rule

Store stable facts, architecture, and operating procedures under `docs/`.
Store repeatable agent workflows under `.agents/skills/`. Keep `AGENTS.md` and
`CLAUDE.md` as compact routers. Workflow scratch state belongs in the ignored
`.plans/` directory and must not become a second documentation tree.
