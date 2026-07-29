---
paths:
  - "**"
---

# Team assembly matrix

When spawning reviewer agents for a change, use this matrix to determine which agents are relevant. Spawn all relevant agents in parallel, collect their verdicts, and report the consensus.

## Consensus rules

- **Ship** = zero BLOCKs and majority APPROVE
- **Fix first** = any BLOCK present (blocker must be resolved)
- **Ship with notes** = zero BLOCKs but one or more CONCERNs

## Assembly by change type

### Rust core changes (`crates/config/`, `crates/types/`, `crates/extract/`, `crates/graph/`, `crates/core/`)

Spawn: `rust-reviewer`

### CLI output changes (`crates/cli/src/report/human/`)

Spawn: `rust-reviewer`, `cli-output-reviewer`

### JSON output changes (`crates/cli/src/report/json.rs`)

Spawn: `rust-reviewer`, `json-output-reviewer`

### CI format changes (`crates/cli/src/report/sarif.rs`, `codeclimate.rs`, `compact.rs`, `markdown.rs`, `badge.rs`)

Spawn: `rust-reviewer`, `ci-formats-reviewer`

### CLI command changes (`crates/cli/src/` excluding `report/`)

Spawn: `rust-reviewer`
If the change affects output formatting, also add the relevant output reviewer(s).

### GitHub Action changes (`action/`, `action.yml`)

Spawn: `github-action-reviewer`

### GitLab CI changes (`ci/`)

Spawn: `gitlab-ci-reviewer`

### MCP server changes (`crates/mcp/`)

Spawn: `rust-reviewer`, `mcp-reviewer`

### LSP server changes (`crates/lsp/`)

Spawn: `rust-reviewer`, `lsp-reviewer`

### VS Code extension changes (`editors/vscode/`)

Spawn: `vscode-reviewer`

### New feature (cross-cutting)

Spawn: `rust-reviewer`, `cli-output-reviewer`, `json-output-reviewer`, `mcp-reviewer`
Optional: `user-panel` for UX/positioning review (advisory only, excluded from APPROVE/CONCERN/BLOCK consensus count)

### New output format or issue type

Spawn: `rust-reviewer`, `cli-output-reviewer`, `json-output-reviewer`, `ci-formats-reviewer`, `github-action-reviewer`, `gitlab-ci-reviewer`, `mcp-reviewer`, `lsp-reviewer`

### Security-sensitive changes (auth, tokens, shell scripts, binary downloads)

Always add: `github-action-reviewer` or `gitlab-ci-reviewer` (whichever is relevant)
Their BLOCK on token exposure or command injection is a hard veto.
