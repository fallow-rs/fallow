# Analyzer Authoring Guide

This guide is for contributors adding a new built-in finding, analyzer, or
framework-specific rule.

## Start With The Contract

Pick the public identity before writing detection code:

- `rule_id`: the stable output rule id, usually `fallow/<code>`.
- `code`: the stable issue code in JSON, SARIF, LSP diagnostics, and suppressions.
- `rules` key: the config key users set to `error`, `warn`, or `off`.
- suppression token: the token after `// fallow-ignore-next-line` or `// fallow-ignore-file`.
- filter flag: only when `fallow dead-code` needs a dedicated selector.
- docs anchor: where `fallow explain` and output formats should point users.

Common contract facts live in `crates/types/src/issue_meta.rs`. Add the row
there first when the finding has a stable issue code, LSP diagnostic code,
filter flag, or suppression token. Keep prose and caveats in the surface that
owns them.

## Implementation Checklist

Use this as the default map for a new finding:

- `crates/types/src/suppress.rs`: add an `IssueKind` only when the finding is suppressible or must persist in cache-facing data.
- `crates/types/src/issue_meta.rs`: add shared code, aliases, labels, config key, filter flag, MCP selector, suppression token, and LSP exposure.
- `crates/config/src/config/rules.rs`: add the rule severity field, aliases, defaults, and unknown-key suggestions.
- `crates/cli/src/explain.rs`: add the `RuleDef`, docs path, guide text, and aliases for `fallow explain`.
- Analyzer code: keep extraction, graph facts, and reporting changes in the narrowest crate that already owns that stage.
- Output formats: verify human, JSON, SARIF, Code Climate, compact, markdown, GitHub, and GitLab consumers when the finding is user visible.
- Actions: add suppress, fix, trace, or config actions when agents can act on the finding safely.
- LSP and MCP: prefer the shared metadata row for contract facts. Keep editor and agent guidance hand-written where nuance matters.
- Schemas and generated types: run `npm run generate:all` after changing generated contract surfaces.
- Docs: update this guide or `docs/plugin-authoring.md` when the workflow changes.

## Detection Shape

Prefer the smallest fact that makes the finding correct:

- Single-file facts stay in extraction or the file-local analyzer.
- Cross-file facts should flow through the module graph or a dedicated analysis pass.
- Framework features should abstain when the framework is absent or the config cannot be resolved confidently.
- Opt-in or warning defaults are appropriate when the public API is open-ended or reflective.
- File-level suppressions fit findings where one source file represents a whole group. Line-level suppressions fit one declaration or usage.

Do not add a new broad abstraction just to group fields by topic. A split is
worth it when it encodes lifecycle or optionality, for example when a plain
JavaScript module structurally cannot carry SFC-only facts.

## Fixture Matrix

Each new analyzer should add fixtures that state what behavior they prove. The
test name or fixture comment should make the proof obvious without reading the
implementation.

| Fixture kind | What it proves |
| --- | --- |
| Positive minimal | The finding appears for the smallest real shape. |
| Negative abstain | The analyzer stays silent when the framework, config, or semantic precondition is missing. |
| False-positive guard | A common nearby pattern does not report. |
| Suppression | The intended line-level or file-level suppression is consumed. |
| Filter and severity | The rule key, CLI filter, and severity gate select the finding correctly. |
| Output contract | JSON actions, SARIF rule id, compact text, and human docs links stay stable. |
| Framework regression | A distilled real-world pattern from a supported framework keeps working. |

Recommended fixture row format for docs, tests, or review notes:

| Analyzer | Fixture | Proof |
| --- | --- | --- |
| `unused-store-member` | `tests/fixtures/pinia-*` | Reports unused Pinia members while reachable store modules stay live. |
| `unused-load-data-key` | `tests/fixtures/sveltekit-*` | Reports unused SvelteKit load keys and keeps sibling data reads live. |
| `invalid-client-export` | `tests/fixtures/invalid-client-export-no-next` | Abstains outside Next.js. |
| `security-sink` | `tests/fixtures/security-*` | Keeps security categories gated by configured matcher and category rules. |

Keep synthetic fixtures small, but include at least one distilled framework
regression when the rule depends on framework conventions.

## Regeneration

After touching schema, generated types, config schemas, plugin schemas, rule-pack
schemas, or agent-facing docs, run:

```bash
npm run generate:all
```

Before opening a PR, run:

```bash
npm run generate:all:check
```

The check command fails on drift without rewriting committed files.
