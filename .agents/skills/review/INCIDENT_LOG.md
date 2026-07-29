### Incident: 2026-06-08, cache path kind drift in companion docs
- **Missed by review**: the companion docs update for `cache.dir` called `.fallow/cache.bin` a directory even though it is the default cache file path.
- **Root cause**: the review caught the missing companion docs coverage but did not re-check path-kind nouns after patching the docs locally.
- **Fix applied**: added a Phase 5 companion-repo checklist item for companion-doc path-kind drift.

### Incident: 2026-06-15, VS Code source formatting missed after drift gate fix
- **Missed by review**: PR #1268 merged with `editors/vscode/src/treeView.ts` failing the JS Lint and Format job even though VS Code build, tests, dist drift, and action drift guards passed.
- **Root cause**: the review skill required VS Code build/dist regeneration for `editors/vscode/src/**/*.ts`, but did not explicitly require the repo-level `npm run fmt:js:check` gate for JS/TS formatter-covered paths.
- **Fix applied**: added a Phase 2a JS/TS format gate checklist item requiring `npm run fmt:js:check` whenever formatter-covered JavaScript or TypeScript paths change.

### Incident: 2026-07-10, Windows-only MCP lint reached main
- **Missed by review**: a needless raw-string hash in an MCP Windows test passed pull-request review and failed Clippy only after merge.
- **Root cause**: workspace Clippy ran on Linux during pull requests, while the Windows Clippy matrix ran only on pushes and merge groups.
- **Fix applied**: Phase 2a now blocks platform-gated Rust changes unless pull requests run Clippy on the matching platform and a workflow policy test preserves the command.

### Incident: 2026-07-11, removed CI job remained a required check
- **Missed by review**: the initial workflow review did not compare removed job names with live `main` branch-protection contexts.
- **Root cause**: the release-workflow checklist covered dependency gates and permissions but not required-status identity outside the YAML files.
- **Fix applied**: Phase 2c-ter now requires live required-check parity and blocks removed contexts until the exact approved repository-rule update is recorded.

### Incident: 2026-07-15, broken PR Markdown escaped review
- **Missed by review**: The schema JSON follow-up PR body displayed literal `\n` text instead of rendered Markdown.
- **Root cause**: The public-artifact audit checked for leaked internal context but did not inspect the live PR body for basic Markdown integrity.
- **Fix applied**: Phase 2b now requires a live PR-body source check and blocks SHIP on escaped newlines or collapsed Markdown structure.

### Incident: 2026-07-20, agent hook and source scanner lacked context
- **Missed by review**: The first implementation of PR #1919 did not honor Claude's repeated Stop-hook flag, missed trailing comments, and classified narration-shaped multiline string content as comments.
- **Root cause**: Review coverage did not explicitly require hook reentrancy or complete-file context for diff-based text gates.
- **Fix applied**: Phase 2a now checks `stop_hook_active`, real hook exit behavior, trailing comments, multiline strings, and full-file validation of diff candidates.

### Incident: 2026-07-22, nested Dependabot workspace opened transitive update flood
- **Missed by review**: PR #1941 added a `/fuzz` Cargo update entry that opened separate pull requests for dependencies inherited through the `fallow-core` path dependency.
- **Root cause**: The review treated Dependabot directory coverage as additive configuration and did not compare direct registry dependencies with the standalone lockfile's transitive root graph.
- **Fix applied**: Phase 2a now requires explicit allow lists for standalone Cargo workspaces whose lockfiles include the root graph through path dependencies, plus a repository policy test.

### Incident: 2026-07-22, failed Zed dependency update auto-merged
- **Missed by review**: PR #1951 merged `ed25519-dalek` 3 even though the focused Zed Extension job failed because version 3 removed the configured `std` feature.
- **Root cause**: The required `CI` aggregate omitted the `zed` job, and dependency triage trusted auto-merge without waiting for the full focused check rollup.
- **Fix applied**: Phase 2a now requires every conditionally runnable focused job to feed `ci-ok.needs`, requires direct inspection of focused checks before auto-merge, and calls for a workflow policy test.
