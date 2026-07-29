### Incident: 2026-05-23, tracked detection rule mirror drift
- **What happened**: Issue #641 updated the ignored `.agents/rules/detection.md` overlay but left the tracked `.claude/rules/detection.md` mirror describing only the old #106 workspace fallback. `$review` caught the drift as a blocker.
- **Root cause**: The implement checklist named the Codex overlay but did not require updating the tracked `.claude` mirror or verifying the two bullets stayed in parity.
- **Fix applied**: Phase 5's detection rule checklist now names both `.claude/rules/detection.md` and `.agents/rules/detection.md`, requires editing them together, and adds a parity check.

### Incident: 2026-05-23, draft PR creation needs a non-empty branch
- **What happened**: Phase 2.0 created and pushed a new branch from `origin/main`, then immediately tried `gh pr create`. GitHub rejected the PR with `No commits between main and fix/issue-630-wrangler-config-precedence` because the branch had no commits yet.
- **Root cause**: The implement skill required opening the draft PR before tracked edits but did not account for GitHub's non-empty-branch requirement.
- **Fix applied**: Phase 2.0 now creates an intentional empty coordination commit before the first push and PR creation, with a note that squash merge will collapse it away after the real implementation commit lands.

### Incident: 2026-06-17, benchmark-only all-targets test run executed long Criterion sampling
- **What happened**: CodSpeed benchmark hardening required benchmark target validation. `cargo test --workspace --all-targets` entered the existing `large_analysis` Criterion suite and began multi-minute 5000-file and duplicate-analysis sampling instead of behaving like a practical test gate.
- **Root cause**: The verification checklist correctly required all-targets coverage for compile and lint, but did not distinguish benchmark target execution from benchmark target compilation.
- **Fix applied**: Phase 3 now has a benchmark-only exception: use `cargo check --benches`, strict clippy with `--all-targets`, targeted `cargo codspeed build/run` for changed benchmark targets, and normal workspace tests with `--lib --bins --tests`.

### Incident: 2026-07-10, Windows-only MCP lint reached main
- **What happened**: A needless raw-string hash in an MCP Windows test passed pull-request checks and failed Clippy only after merge.
- **Root cause**: The implementation checklist required workspace Clippy but did not require a pull-request Clippy job on the platform that compiles `#[cfg(windows)]` code.
- **Fix applied**: The CI-prevention checklist now requires platform-matched pull-request Clippy and a workflow policy test whenever platform-gated Rust changes.

### Incident: 2026-07-11, removed CI job remained a required check
- **What happened**: The release-only cross-platform CI plan removed `Windows ARM64 Native Compile` from regular CI while live `main` branch protection still required that exact context.
- **Root cause**: Planning inspected workflow dependencies but did not compare removed job names against the repository's live required status checks.
- **Fix applied**: Phase 1 impact assessment now requires live required-check parity, exact repository-rule scope, preservation of unrelated contexts, and explicit approval before changing branch protection.

### Incident: 2026-07-14, identical mirrored bullets bypassed parity
- **What happened**: The same Next.js detection bullet appeared twice in both `.claude/rules/detection.md` and `.agents/rules/detection.md`. The documented mirror diff still exited successfully.
- **Root cause**: The check proved that the mirrors matched but did not prove that each bullet was unique.
- **Fix applied**: Phase 5 now requires an exact-one count for the bullet title in both mirrors before comparing their content.

### Incident: 2026-07-15, PR body rendered escaped newlines literally
- **What happened**: The schema JSON follow-up PR body displayed `\n` text instead of Markdown line breaks after an update.
- **Root cause**: The initial PR creation used `--body-file`, but the later body-update guidance did not require the same safe path or a live source check.
- **Fix applied**: Phase 2.0 now requires `gh pr edit --body-file` for multiline updates and immediate verification with `gh pr view --json body --jq .body`.

### Incident: 2026-07-20, comment guard missed hook and source context
- **What happened**: PR #1919 initially let repeat Claude Stop hooks block indefinitely, missed trailing narrator comments, and treated narrator-shaped text inside multiline strings as comments.
- **Root cause**: The implementation tested line regexes without the hook reentrancy contract or complete staged-file lexical context.
- **Fix applied**: Phase 4 now requires `stop_hook_active` handling, trailing-comment coverage, multiline-string regression cases, full-file validation of diff candidates, and real Git integration tests.

### Incident: 2026-07-21, cross-surface audit and context hardening gaps
- **What happened**: PR #1941 review found stable-key collision count drift, missing API and MCP styling gates, workflow-command message injection, unsafe or malformed fallback paths, nested-workspace context gaps, destination symlink traversal, unbounded and raceable fingerprint reads, and an unstable Windows metadata API.
- **Root cause**: Phase 4 covered individual output and Rust surfaces but did not require a collision/domain parity matrix, hostile-data fallback matrix, or filesystem-security review for materialized audit context.
- **Fix applied**: Phase 4 now requires collision-safe CLI/API/MCP audit parity, hostile workflow-command tests across every fallback mode, and bounded cross-platform no-follow materialization and fingerprinting checks with adversarial filesystem regressions.

### Incident: 2026-07-22, audit styling omitted by downstream aggregators
- **What happened**: PR #1941 follow-up review found that styling attribution was correct in CLI, API, and MCP JSON, but the GitHub Action issue count, GitHub and GitLab audit summaries, and VS Code gating display still omitted styling-only failures. A null styling path also crashed the jq fallback renderer, and the editor rendered an incorrect singular label.
- **Root cause**: The audit parity checklist stopped at producers and typed contracts. It did not require tracing every downstream scalar aggregation and renderer that independently counts or displays audit domains.
- **Fix applied**: Phase 4 now requires a consumer sweep across Action scripts, provider jq templates, and VS Code, with styling-only `new-only` and `all` fixtures, severity-aware gating, null-safe rendering, and count-aware labels.

### Incident: 2026-07-22, changed workload polluted a historical CodSpeed identity
- **What happened**: PR #1941 temporarily changed `component_engine_warm_session_css_health` from its historical fixture to a 96-file fixture without changing the benchmark ID. CodSpeed reported a false `+87.56%` jump on the intermediate commit. The final fix restored the original benchmark and added `component_engine_warm_session_css_health_many_files` for the larger workload.
- **Root cause**: The benchmark checklist verified compilation and execution but did not require stable benchmark identity when fixture shape or scale changed.
- **Fix applied**: Phase 3 now blocks reusing an existing benchmark URI or name for a materially different workload, requires a merge-base comparison of fixture cardinality, and requires checking the first CodSpeed graph for discontinuities.
