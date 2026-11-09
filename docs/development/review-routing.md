# Review routing

Apply the review lenses that match the changed paths.

## Path routing

| Changed area | Primary review lens |
|---|---|
| Crate dependencies, shared protocols, output contracts | Architecture invariants, dependency direction, serialization boundary |
| `crates/config`, `types`, `extract`, `graph`, `core`, `engine` | Rust correctness, accuracy, performance, memory, paths |
| `crates/security` or security analyzers | Candidate precision, evidence safety, catalogue integrity, agent verification |
| `crates/license`, runtime coverage, public protocol | Offline verification, version parity, privacy, failure modes |
| `crates/napi`, `crates/multicall`, npm packaging | API parity, binary dispatch, generated types, package contents |
| Human CLI output | Scanability, hierarchy, empty states, terminal compatibility |
| JSON output | Schema stability, actions, determinism, null versus absent |
| SARIF, CodeClimate, compact, Markdown, badge | Specification compliance, severity, stable identifiers, relative paths |
| MCP | Tool contracts, parameters, structured errors, safe actions, timeouts |
| LSP | Protocol compliance, diagnostics, edits, workspace-root selection, single-root analysis |
| VS Code | Binary resolution, lifecycle, lazy work, configuration, UX |
| GitHub Action, GitLab CI | Shell quoting, tokens, jq, annotations, comments, Code Quality |
| Visualization | Data minimization, browser security, accessibility, large-project behavior |
| Release | Version and changelog parity, artifacts, signatures, registries, companion contracts |
| Docs, skills, adapters | Canonical ownership, fresh-clone discovery, drift, privacy |

Start cross-crate reviews with
[architecture invariants](../architecture-invariants.md). Use
[the repository map](repo-map.md) to locate the producer and every downstream
consumer.

## Review priorities

1. Correctness and false-positive or false-negative risk.
2. Public output and integration contract stability.
3. Shell, token, path, and privacy safety.
4. Cross-platform behavior.
5. Performance in parse, graph, and analysis hot paths.
6. Documentation, generated adapter, and companion-surface parity.

For a change to signal handling, process cleanup, or other timing-sensitive
code, run the `Coverage` workflow on the pull request branch before the merge:
`gh workflow run coverage.yml --ref <branch>`. `Coverage` runs only on `main`,
and its slow instrumented build finds races that the pull request jobs miss.

For a change to a pre-parse guard (`crates/extract/src/nesting.rs`,
`crates/config/src/toml_input.rs`), review the guard against the lexer of the
parser that it protects. A guard that reads strings, comments, escapes, or
regex literals in a different way lets an input pass the guard and then
overflow the parser. Check these items:

- Each token form that can hide a bracket or a `<` from the guard.
- A test for each bypass, plus the original fuzz crash file.
- A run of the matching `fuzz/` target on the crash files.

## Synthesis

- `Fix first` when any high-confidence blocker remains.
- `Ship with notes` for concrete non-blocking concerns.
- `Ship` only when no meaningful concern remains.

Report findings first with paths and severity. Verification output is evidence
only when the command covers the claimed requirement.
