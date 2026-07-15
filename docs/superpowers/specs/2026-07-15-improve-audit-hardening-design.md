# Improve Audit Hardening Design

## Scope

This change resolves the nine confirmed improve-audit findings that do not involve `fallow fix`. It covers analyzer correctness, cached CSS analysis allocation, GitHub Action trust boundaries and current-binary coverage, VS Code real-process coverage, repository toolchain reproducibility, compatibility wording, and output-format documentation.

The `fallow fix` promotion race and optional product directions are not part of this change.

## Architecture

The implementation keeps each concern in its existing ownership boundary:

- Core analyzer code owns nested-workspace dependency attribution.
- The engine session owns sharing of immutable styling artifacts.
- Action shell code owns validation of Action inputs before Git or GitHub file-command use.
- Repository CI owns locked validators and current-binary integration gates.
- The VS Code extension owns its real-process contract smoke.
- Source docs and generators jointly own compatibility and environment-variable contracts.

The findings share one branch and PR, but each behavior change remains independently testable and reviewable.

## Analyzer correctness

Both dependency usage indexes will select the deepest workspace root that contains a module. One shared helper will define that ownership rule so the direct unused-dependency check and `used_in_workspaces` metadata cannot diverge again.

A recursive-workspace fixture will declare the same dependency in an ancestor and nested package while importing it only from the nested package. The expected result is that the ancestor declaration remains unused and the nested declaration is credited.

## Styling artifact cache

`AnalysisSession` will cache immutable styling artifacts as `Arc<StylingAnalysisArtifacts>`. Warm consumers receive clones of the `Arc`, not deep clones of the artifact graph. Report-local mutable state remains independently cloned where mutation is required.

An identity test will pin allocation sharing. A CSS-enabled warm-session benchmark and real-project output comparison will prevent an allocation optimization from silently changing behavior.

## Action trust boundary

The Action will treat `changed-since` and `diff-file` as validated scalar inputs before they reach Git or GitHub file-command files. Revisions beginning with `-` are invalid because Git can interpret them as options. ASCII control characters are invalid because line-oriented GitHub files cannot safely encode them as scalar values.

Validation errors use a stable Action error annotation and exit code 2. Quoted valid refs and paths containing spaces remain supported.

## Reproducible CI and current-binary coverage

Intent validation will use the root lockfile and a local no-install invocation. A policy test will reject versioned network `npx` validation.

The published-package Action workflow remains as a compatibility gate. A separate bounded CI lane will build the current Rust binary, run the Action shell suite with explicit `FALLOW_BIN`, and exercise one checked-in composite Action JSON path.

## VS Code contract coverage

Fast tests using fake CLI and LSP processes remain for deterministic extension behavior. A separate real-process smoke will launch the current `fallow` binary for both CLI output and LSP diagnostics, proving the extension and Rust contracts agree. CI path filters will include the Rust crates that own these protocols.

## Compatibility and tooling contracts

Legacy output aliases remain supported throughout v3. Removing them requires an explicit deprecation period and a future major release. The hand-written policy, generator text, and generated declaration files will use the same wording.

Root repository tooling requires Node `>=22.12.0`, matching its locked development tools. This does not change the published CLI package's runtime floor.

The `FALLOW_FORMAT` documentation will include `github-annotations` and `github-summary`. A drift assertion will tie the documented list to the accepted format catalog.

## Error handling

No analyzer or output schema error contract changes. Action validation fails before side effects and uses the existing runtime-error exit convention. CI and editor smoke failures remain ordinary test failures with focused diagnostic output.

## Verification

Every behavior change begins with a failing regression test. Verification then expands through focused crate and integration tests, Action and editor real-process smoke, generated-contract checks, a real recursive-workspace comparison, a real CSS-heavy project comparison, full repository verification, and area-specific review.
