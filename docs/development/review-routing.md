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
| GitHub Action | Shell quoting, tokens, jq, annotations, comments |
| GitLab CI | Shell quoting, tokens, Code Quality, MR comments |
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

## Synthesis

- `Fix first` when any high-confidence blocker remains.
- `Ship with notes` for concrete non-blocking concerns.
- `Ship` only when no meaningful concern remains.

Report findings first with paths and severity. Verification output is evidence
only when the command covers the claimed requirement.
