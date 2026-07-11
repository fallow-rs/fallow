# Release-only cross-platform CI design

## Status

Approved in conversation on 2026-07-11. This document is the written design checkpoint before implementation planning.

## Goal

Keep pull request and `main` CI as fast as reasonably possible without weakening the checks that provide rapid, high-value feedback. Run regular CI on Ubuntu only. Move expensive Windows and macOS correctness checks into the release workflow, where every cross-platform gate must pass before any package is published or release is created.

## Current problem

Regular CI currently expands some jobs on pushes to `main`:

- The primary Rust and NAPI `check` job adds `windows-latest`.
- The Zed extension job adds macOS and Windows.
- Dedicated Windows ARM64 compilation and Windows audit lifecycle smoke jobs run in CI.

The Windows primary check can take more than 30 minutes. This slows completion of `main` CI even though the release workflow already builds Windows and macOS artifacts. The release workflow does not yet replace all correctness coverage that would be removed from regular CI.

## Proposed workflow split

### Regular CI

`.github/workflows/ci.yml` will use Ubuntu runners only.

The primary `check` job will become a single Ubuntu job. It will retain the existing high-value suite:

- Workspace tests, examples, and runtime coverage.
- Schema, generated contract, and documentation drift checks.
- Clippy for the default workspace and feature combinations.
- Rust formatting.
- NAPI dependency installation, addon build, declaration generation, package smoke tests, and generated-file drift checks.

Its timeout will be reduced from the Windows-sized 40-minute budget to 20 minutes. This leaves headroom above normal Ubuntu runtime while still detecting hangs promptly.

The Zed job will run on Ubuntu only for pull requests, merge groups, and pushes to `main`. The conditional Windows and macOS matrix will be removed.

The following dedicated cross-platform jobs will be removed from regular CI:

- Windows ARM64 native compilation.
- Windows audit process and lock lifecycle smoke tests.

The aggregate required `CI` job will no longer depend on Windows-only jobs. Branch protection will continue requiring the stable aggregate `CI` check without waiting for cross-platform runners. The obsolete separately required `Windows ARM64 Native Compile` context will be removed from branch protection after the workflow lands; every other required context remains unchanged.

Other Ubuntu jobs and specialized scheduled or manual workflows are unchanged.

### Release verification

`.github/workflows/release.yml` will remain the cross-platform artifact build workflow. Its existing matrix will continue building the CLI, LSP, MCP server, and NAPI addon for supported Linux, macOS, Windows x64, and Windows ARM64 targets.

Release will add unprivileged verification jobs before publishing:

1. A Windows x64 correctness job will run the coverage moved out of regular CI:
   - Workspace tests and examples.
   - Runtime-coverage tests.
   - Schema and generated-contract drift checks.
   - Default and feature-specific Clippy checks.
   - Rust formatting.
   - NAPI install, native build, declaration generation, package smoke tests, and generated-file drift checks.
   - Focused audit lock, orphan cleanup, process-tree, and Windows lifecycle tests.

2. A release-only Zed matrix will run the extension tests, build, and formatting checks on macOS and Windows. Ubuntu Zed coverage remains in regular CI and does not need to be repeated during release.

The existing Windows ARM64 release matrix entry remains responsible for native Windows ARM64 compilation and NAPI addon production.

Verification jobs will have only read access to repository contents. They will not receive publishing credentials, OIDC permissions, or release tokens.

## Release dependency gate

All privileged or externally visible release paths must depend on the cross-platform verification jobs, either directly or through a single explicit release gate. This includes:

- crates.io publication.
- npm package publication.
- GitHub release creation and artifact attachment.
- Any downstream release completion or notification job that assumes publication succeeded.

The implementation plan will map the existing dependency graph and choose the smallest clear dependency change. The preferred shape is one unprivileged aggregate verification job that depends on artifact builds, Windows correctness, and release-only Zed checks. Publish jobs then depend on that gate. A failing cross-platform check must stop the release before credentials are used or packages become visible.

## Policy tests

`scripts/workflow-policy.test.mjs` will enforce the split structurally:

- Regular CI must not use Windows or macOS runners for the primary check, Zed, Windows ARM64, or lifecycle coverage.
- The regular primary check must retain its important Ubuntu commands and its reduced timeout.
- Release must retain Windows x64 and Windows ARM64 artifact builds.
- Release must contain the Windows correctness, lifecycle, and NAPI smoke commands moved from CI.
- Release must contain macOS and Windows Zed verification.
- Every privileged publish or release path must depend on the aggregate release verification gate.
- Verification jobs must remain unprivileged.

Tests will be added before workflow changes so the initial failure proves the new policy is not already satisfied.

## Failure behavior

- Pull request and `main` failures remain fast and Linux-focused.
- Cross-platform regressions fail the release workflow before publication.
- A failed release verification can be fixed on `main` and retried with a new release tag according to the existing release process.
- The workflow will not allow a verification failure to be skipped by a successful matrix sibling or an unrelated publish dependency.

## Scope boundaries

This change does not:

- Change product code or supported platforms.
- Remove any release artifact target.
- Add third-party dependencies.
- Change specialized scheduled or manually dispatched workflows.
- Change branch protection beyond removing the obsolete `Windows ARM64 Native Compile` context and preserving every other required check.
- Redesign the release process outside the dependency changes required to gate publication.

## Acceptance criteria

- Pull request, merge-group, and `main` CI use Ubuntu only for the affected regular checks.
- The aggregate `CI` check remains stable and includes all regular required jobs.
- Branch protection no longer requires the removed `Windows ARM64 Native Compile` job and retains every other required context.
- Release builds all currently supported platform artifacts.
- Windows correctness, lifecycle, and NAPI smoke checks run before publishing.
- macOS and Windows Zed checks run before publishing.
- No publish or GitHub release path can start until cross-platform verification passes.
- Workflow policy tests fail on the old structure and pass on the new structure.
- YAML validation, workflow policy tests, and relevant local verification pass.
