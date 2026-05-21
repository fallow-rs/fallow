# Changelog

All notable changes to fallow are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **The LSP no longer publishes diagnostics that reference lines you no longer wrote.** Before, the language server captured analysis input when `did_save` fired but published the result unconditionally, even when the user typed further during the (potentially long) analysis run. Squiggles ended up pinned to lines from the pre-edit document and `code_action` suggestions referenced identifiers that had moved or disappeared. After, the LSP snapshots the per-URI document `version` at the entry of every analysis run, threads the snapshot through to the publish path, and per URI: when the live version has advanced past the snapshot (or the user closed the file during analysis) the publish AND the pull-model cache update are skipped, preserving the last-known-valid diagnostics on the client until the next analysis catches up. Every other publish now carries `Some(version)` in `PublishDiagnosticsParams.version` so LSP 3.17 clients (VS Code, Helix, Zed, Neovim 0.10+) can drop already-superseded notifications client-side as a belt-and-braces second line of defense. The single-flight `analysis_guard`, the 500ms `did_save` debouncer, the cancellation flag, and the pull-model `textDocument/diagnostic` handler are unchanged. (Closes [#450](https://github.com/fallow-rs/fallow/issues/450).)

- **Ctrl+C and SIGTERM now reap fallow's spawned subprocesses instead of leaking them.** Before, fallow installed zero signal handlers, so long-running children kept executing after the parent exited: the `fallow-cov` sidecar (can run for minutes on large coverage dumps), `npm install -g @fallow-cli/fallow-cov`, the self-invoked `fallow health --runtime-coverage` from `fallow coverage setup`, the `git log --numstat` churn-analysis subprocess used by `fallow health --hotspots`, the `git rev-parse` / `git diff` / `git ls-files` subprocesses used by `--changed-since` and the LSP's `changedSince` filter, and the `git worktree add` / `git worktree remove` operations used by `fallow audit`'s base-snapshot path all kept executing in the background. On self-hosted CI runners, cancelled jobs accumulated tens of zombie `fallow-cov` processes per week. After, every long-running spawn site is wrapped in a process-wide `ScopedChild` RAII registry; a single signal handler installed at `main()` entry kills every registered child on SIGINT (exit 130), SIGTERM (exit 143), or the Windows `CTRL_C_EVENT` / `CTRL_BREAK_EVENT` / `CTRL_CLOSE_EVENT` / `CTRL_LOGOFF_EVENT` / `CTRL_SHUTDOWN_EVENT` console-control events. `fallow watch` is the documented Ctrl+C consumer and exits cleanly with code 0 because the user-initiated stop is the intended termination path; in graceful mode the handler still drains registered children so a Ctrl+C delivered DURING `analyze_and_report` reaps in-flight git subprocesses instead of letting them complete. Every other CLI command exits with the conventional POSIX 128+signum. The LSP server's `shutdown` request now cancels in-flight diagnostic publishes via an `Arc<AtomicBool>` so a closing editor stops receiving updates; rayon work already running on the blocking thread pool runs to natural completion and its results are dropped (`spawn_blocking` is not interruptible, documented as a known limitation). On Unix the signal listener runs in a dedicated `std::thread` doing a blocking `sigwait` via `signal_hook 0.4`, which sidesteps async-signal-safety constraints; on Windows the kernel-spawned handler calls `TerminateProcess` directly via `windows-sys`. (Closes [#477](https://github.com/fallow-rs/fallow/issues/477).)

- **`.fallow/cache.bin` now enforces its size cap on every save and invalidates automatically when extraction-affecting config changes.** Two structural gaps in the incremental extraction cache are closed in one pass. (1) The 256 MB ceiling was checked only at load time; once a long-running developer machine crossed the threshold, the next run silently discarded the entire cache and re-extracted everything cold. `CacheStore::save` now encodes once, evicts the oldest entries (by `last_access_secs` ascending, path-tiebroken for reproducible order across runs) when the encoded size crosses 80% of the cap, and re-encodes to write below 60%. The cap is overridable per-project via `cache.maxSizeMb` in `.fallowrc.json` or the `FALLOW_CACHE_MAX_SIZE` env var (env var wins; both interpret the value as whole megabytes; default 256 MB). Atomic-rename writes (`cache.bin.tmp` -> `cache.bin`) close the partial-truncate window the previous direct-`std::fs::write` exposed. Eviction logs at `tracing::debug!` by default and promotes to `tracing::info!` only when at least 25% of entries are removed in a single save, so warm-cache saves stay silent. (2) The cache key was `(file_path, content_hash)` plus a global `CACHE_VERSION` constant: disabling a plugin, removing an inline `framework: [...]` entry, or renaming a `.fallow/plugins/foo.json` left the cache stale until the next CACHE_VERSION bump. The cache header now carries a stable `config_hash: u64` (xxh3 over a sorted list of active external plugin names plus inline framework definition names) and a mismatch on load discards the cache, matching the version-mismatch path's shape. Narrow ingredient set by design: `entry`/`ignorePatterns` are detection inputs (not extraction inputs), so editing them does not bust the cache. The hash skips its computation entirely under `--no-cache`. `CACHE_VERSION` bumps from 87 to 88 (new `last_access_secs` field on `CachedModule`, new header field on `CacheStore`); existing warm caches discard automatically on upgrade with a one-line `Cache format upgraded, rebuilding (one-time cost after version bump)` info log. See [ADR-009](https://github.com/fallow-rs/fallow/blob/main/decisions/009-cache-config-hash-ingredients.md) for the contract on adding new ingredients in the future. (Closes [#466](https://github.com/fallow-rs/fallow/issues/466).)

- **`// fallow-ignore-*` markers for a rule that is currently disabled (`rules.<kind> == "off"`, including per-file `overrides.rules`) no longer surface as `stale-suppression` findings.** Before, when a project set `rules.unused-exports = "off"` and kept a `// fallow-ignore-next-line unused-export` above a declaration, the detector skipped emission entirely (no `unused-export` finding) but the suppression's `used` flag stayed false, so `find_stale` reported the marker as stale. Users were told to remove a comment that documented intentional dormancy and was valid again the moment the rule was re-enabled. After, `SuppressionContext::find_stale` resolves rules once per file via `ResolvedConfig::resolve_rules_for_path` (per-file `overrides.rules` apply uniformly) and skips emission when the suppression's target kind is `Severity::Off`. Blanket markers (`// fallow-ignore-next-line` with no kind) are not anchored to any specific dormant kind and continue to surface as stale when nothing matches. Multi-kind markers (`// fallow-ignore-next-line unused-export, unused-type`) already track each kind as a separate `Suppression` entry, so a dormant clause in a list with a live clause now stays silent without affecting the live clause. CI baselines that contain these stale findings (from the prior false-positive behavior) will see them disappear on the next run, a strictly noise-reducing delta with no API or schema change. (Closes [#482](https://github.com/fallow-rs/fallow/issues/482).)

- **Suppression markers with an unknown token now keep the recognized tokens working and surface the unknown one as a `stale-suppression` finding.** Before, `// fallow-ignore-next-line unused-export, complexity-typo` was silently discarded in full because one token did not parse; `unused-export` was NOT suppressed and no diagnostic surfaced. After, the recognized `unused-export` suppression still applies, and the typo (or obsolete kind name) appears as a `stale-suppression` carrying `kind_known: false` on the wire. The human-facing explanation reads `'<token>' is not a recognized fallow issue kind. Did you mean '<closest>'? Other tokens on this line still apply.` with a Levenshtein "did you mean?" hint when an existing kind is within edit distance 2. Issue-kind names are not stable forever: kinds get added, renamed, and removed across fallow releases, so a marker mentioning an old name no longer silently loses the rest of its tokens after an upgrade. JSON consumers (CI annotations, MCP agents, VS Code) can branch on the new `origin.kind_known` field to render typo-fix copy instead of "no longer matches any active issue." The bundled GitHub Action and GitLab CI templates have been updated to honor this. The extraction cache (`CACHE_VERSION` bumped from 86 to 87) invalidates on upgrade so warm `.fallow/cache` directories pick up the change without manual clearing. (Closes [#449](https://github.com/fallow-rs/fallow/issues/449).)

### Added

- **New `ignoreDecorators` config option lets you opt specific decorators out of the default `unused-class-members` skip-all-decorated behavior.** Before, every class member carrying any decorator was skipped to avoid false positives on framework-managed members (NestJS `@Get()`, Angular `@Input()`, TypeORM `@Column()`). After, a class library can list non-reflective utility decorators (Playwright `@step`, internal `@measure`, `@log`, etc.) so methods decorated with ONLY those names are checked for usage like undecorated methods. Conservative semantics: a method carrying any decorator NOT in the list stays skipped, so a `@step` + `@Inject` combination is still treated as framework-managed.

  ```jsonc
  // .fallowrc.json
  {
    "ignoreDecorators": ["@step"]
  }
  ```

  Matching rule: entries containing `.` (`"decorators.log"`) match the full dotted path; bare entries (`"step"` or `"decorators"`) match the leftmost segment, so a single bare `"decorators"` entry collapses an entire `@decorators.*` namespace. Both `"@step"` and `"step"` round-trip equivalently (a leading `@` is stripped before matching). Unmatched entries (a decorator name in the config that never appears in the analyzed codebase) emit a one-time `tracing::warn!` at end of run, mirroring the existing `usedClassMembers` behavior. The default empty list preserves today's skip-all behavior, so existing NestJS / Angular / TypeORM projects see no change. The first run after enabling this option will surface new `unused-class-members` findings on members previously hidden by the unconditional skip. The extraction cache (`CACHE_VERSION` bumped from 84 to 85) invalidates automatically on upgrade so users with warm `.fallow/cache` directories get the fix without manual clearing. Thanks [@vethman](https://github.com/vethman) for the report. (Closes [#471](https://github.com/fallow-rs/fallow/issues/471).)

### Security

- **`npm install fallow` and the `fallow-rs/fallow` GitHub Action now verify Ed25519 signatures and GitHub Release SHA-256 digests on every platform binary before running it.** Each `@fallow-cli/<platform>` npm package ships `.sig` files alongside its `fallow`, `fallow-lsp`, and `fallow-mcp` binaries (produced at release time by the workflow's Ed25519 signing step). The npm postinstall script verifies all three signatures using a public key embedded in `npm/fallow/scripts/verify-binary.js`, then fetches the matching GitHub Release `asset.digest` and compares the binary SHA-256. A tampered binary aborts the install with exit code 1 and a structured `fallow: binary verification failed: ... (sig-invalid|sig-missing|binary-missing|digest-mismatch|digest-unavailable)` message. The GitHub Action installer (`action/scripts/install.sh`) installs with `--ignore-scripts`, then runs the verifier from the checked-out Action code instead of from the just-installed npm package, so CI runners do not execute package lifecycle scripts before verification. npm release publishing now hard-fails if `npm publish --provenance` fails. Set `FALLOW_SKIP_BINARY_VERIFY=1` only when deliberately replacing the published binary (source builds, airgapped registries, signed mirror repacks); see `SECURITY.md` for the public key fingerprint and the manual out-of-band verification recipe. (Closes [#465](https://github.com/fallow-rs/fallow/issues/465).)

### Changed

- **`fallow` now exits with code 2 at config load when a `boundaries.rules[]` entry references a zone not present in `boundaries.zones[]`, or when a zone pattern redundantly repeats its own `root` prefix.** Before, `crates/config/src/config/resolution.rs` emitted a `tracing::error!` and continued; the rule remained active with a phantom target, and `is_import_allowed` returned `false` for every import to the non-existent zone, producing a flood of false-positive boundary violations like "ui cannot import from typo-zone." The `tracing::error!` was invisible in normal operation, so a typo'd zone name or a stale `allow:` reference shipped as analysis output instead of a config-load failure. After, fallow validates references AFTER preset expansion and auto-discover (so Bulletproof's `features` logical-group rule resolves correctly) and lists every offending `(rule_index, field, zone_name)` tuple in one rendered diagnostic. The redundant-root-prefix path (`packages/app/src/**` inside a zone with `root: "packages/app/"`) also exits 2; the legacy `FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX` tag is preserved in the Display rendering so existing CI grep recipes continue to work. The invalid-glob-pattern half of #468 was already closed by [#486](https://github.com/fallow-rs/fallow/issues/486) (`validate_user_globs` covering `boundaries.zones[].{patterns, root, autoDiscover}`); this completes the issue. (Closes [#468](https://github.com/fallow-rs/fallow/issues/468).)

- **`fallow migrate` now warns on knip rule / `exclude` / `include` keys it cannot translate, and prints a glob-drift caveat when `entry` or `ignorePatterns` are migrated.** Before, three classes of knip input silently disappeared from the generated fallow config: (a) rule keys neither in `KNIP_RULE_MAP` nor in `KNIP_UNMAPPABLE_ISSUE_TYPES` (typos, future knip rules, internal rules the migrator has not yet catalogued) were dropped without diagnostic; (b) `exclude` and `include` entries shared the same silent-drop bug; (c) `entry` / `ignorePatterns` globs were copied verbatim with no statement about knip's glob engine vs fallow's `globset` potentially diverging on corner cases (negation order, brace expansion, leading `./`, double-star semantics). After, `crates/cli/src/migrate/knip_fields.rs::warn_unmapped_rule_key` emits a `MigrationWarning` for every untranslated key under all three fields, with two shapes: documented-unmappable issue types keep the existing message (`issue type \`X\` has no fallow equivalent`, no suggestion); completely-unknown keys carry a new message (`unknown knip issue type \`X\`; not migrated`) with a suggestion pointing at `https://docs.fallow.tools/migration/from-knip` so users can fix the typo or report the missing mapping. `crates/cli/src/migrate/mod.rs::run_migrate` appends a one-line `Note:` block after the warnings section when knip was a migration source AND the migrated config carries `entry` or `ignorePatterns`, telling users to run `fallow check` on a subset before relying on the migration in CI. The note is suppressed for jscpd-only migrations and for rules-only knip configs. The warnings header is now singular/plural-aware (`Warning (1 skipped field)` vs `Warnings (3 skipped fields)`) since a single typo'd rule is the most common count-one case. Content-detected `--from custom-name.json` paths whose filename does not contain `knip` are now tagged internally with `(knip config)` so the glob caveat still fires through that branch; the tag is stripped via a `source_head` helper before printing the "Migrated from:" line and before writing the `// Migrated from ...` header to the generated config, so the suffix is never user-visible. A new end-to-end integration test in `crates/cli/tests/migrate_tests.rs` builds a Next.js-shaped fixture project (`app/`, `components/`, `lib/`, `pages/` plus excluded `__tests__/`, `dist/`, `node_modules/`, `scripts/`), runs the migrator on a representative knip config, then runs `fallow list --files` against the migrated config and asserts the in-scope file set exactly matches a hand-recorded ground truth derived from knip's documented glob semantics. New unit tests cover the unknown-key warning at `rules` / `exclude` / `include` paths, the glob-caveat gating across knip / jscpd / combined sources, the `source_head` helper (including path-with-parens and unclosed-paren edge cases), `OutputFormat::pick` auto-mirror through tagged sources, and a small set of `knip_glob_equivalence_*` assertions documenting the patterns where both engines agree today (brace expansion, `**` cross-segment, `src/**`, `?`) plus one negative case (`!keep.ts` is taken literally under `globset`; fallow `ignorePatterns` does not support `!` negation). (Closes [#457](https://github.com/fallow-rs/fallow/issues/457).)

- **`fallow` now exits with code 2 at config load when a glob pattern is invalid, absolute, or contains a `..` segment.** Affected config fields: `entry`, `ignorePatterns`, `dynamicallyLoaded`, `duplicates.ignore`, `health.ignore`, `overrides[].files`, `ignoreExports[].file`, `ignoreCatalogReferences[].consumer`, `boundaries.zones[].{patterns, root, autoDiscover}`, every glob-bearing field on inline `framework[]` plugin definitions, and every glob-bearing field on external plugin files discovered from `.fallow/plugins/`, root-level `fallow-plugin-*.{toml,json,jsonc}`, or the `plugins:` config list (including `detection.fileExists.pattern` and nested `all` / `any` combinators). Before, invalid patterns silently no-op'd or warn-and-skipped, so a typo in `entry` could leave a user wondering why nothing was being analyzed. After, fallow lists every offending field + pattern in one go and tells you what to type instead. Closes the path-traversal surface flagged for malicious configs shipped via PR on misconfigured CI runners. Configs that ran with silently-dropped patterns must be fixed to upgrade; the dropped patterns were never doing what their author intended. (Closes [#463](https://github.com/fallow-rs/fallow/issues/463).)

### Fixed

- **`fallow fix` and any other writer routed through `fallow_config::atomic_write` now preserve the target file's existing Unix file mode on Unix instead of silently downgrading it to `0600`.** Before, `NamedTempFile` produced the temp at the tempfile-crate default of `0600`; `persist` swapped the temp into the target's path, replacing the target's mode with the temp's. A source file at `0644` (the typical mode after `git checkout`, editor save, or `tar` extract) ended up at `0600` after `fallow fix`, breaking shared workspaces and CI runners that relied on group / other read permissions. The bug was pre-existing in the `atomic_write` shape and inherited by the issue #454 batched-commit refactor. After, both `fallow_config::atomic_write` and the issue-#454 `FixPlan::commit` call a new `fallow_config::preserve_target_mode(temp, target)` helper between the temp write and the rename: when the target exists, its mode is copied onto the temp; when the target does not yet exist (fresh creation, e.g. the duplicate-exports config-add fallback writing `.fallowrc.json` for the first time), the temp's default mode is left in place. Windows ACLs already persist with the underlying file across `MoveFileEx`, so the helper is a no-op there. Three new regression tests in `crates/config/src/config_writer.rs` and `crates/cli/src/fix/plan.rs` pin the contract.

- **`fallow check --regression-baseline <path>` now exits 2 with an actionable regenerate hint when the baseline's `schema_version` does not match this fallow build, instead of silently loading default-zero fields and producing a wrong "no regression detected" verdict.** Before, `crates/cli/src/regression/baseline.rs::load_regression_baseline` called `serde_json::from_str` with no version check, and `CheckCounts` carries `#[serde(default)]` on every count field. A baseline written by a different `REGRESSION_SCHEMA_VERSION` (today's value: `1`) silently deserialized with missing fields defaulting to zero, masking real regressions and letting CI gates pass on a structurally invalid baseline. After, the loader validates `baseline.schema_version == REGRESSION_SCHEMA_VERSION` immediately after parse and fails with `regression baseline '<path>' has schema_version <actual> but this fallow build expects <expected>. The baseline was written by fallow <writer-version>. Regenerate it by running: fallow check --save-regression-baseline <path>`. A baseline with `schema_version: 0` (predates versioning) and a baseline missing the field entirely both surface the same hint reworded for their case. Existing baselines with `schema_version: 1` continue to load unchanged; the constant has not changed. Future schema bumps require regenerating the baseline (re-run the save command), and downgrading fallow across a schema bump also requires regenerate. The three existing error paths in `load_regression_baseline` (file not found, read error, JSON parse error) plus the new schema-mismatch path now route through `emit_error`, so `--format json` CI consumers receive a structured `{"error": true, "message": "...", "exit_code": 2}` envelope on stdout instead of human text on stderr. (Closes [#451](https://github.com/fallow-rs/fallow/issues/451).)

- **`fallow fix` now refuses to overwrite a file that changed between analysis and the per-file write, and batches its writes so a mid-run failure leaves the project untouched.** Before, `fallow fix` re-ran analysis in-process, then wrote each per-file rewrite via a standalone `atomic_write`. If a parallel editor save, CI rebase, or other tool mutated a target file between the analysis read and the fix-time write (window: seconds to minutes), the line offsets computed during analysis landed on the wrong bytes and silently corrupted the file. Per-file writes also ran unbatched, so an OOM, disk-full, or transient I/O error mid-run left the project in a half-applied state where the surviving files' offsets no longer matched the original analysis. After, every parsed source file's xxh3 content hash (the same hash the extract cache already computes) is captured during analysis and threaded into each fixer; per-file entry recomputes the hash and skips with a clear `Skipping <path>: file content changed since fallow check ran. Re-run fallow fix to refresh the analysis first.` diagnostic on mismatch. Each fixer stages its rewrite into a sibling `NamedTempFile` instead of writing directly; the orchestrator commits the plan only after every stage has succeeded, so a single stage failure leaves the project untouched (POSIX has no atomic multi-rename primitive, so a rename failure mid-commit reports per-path while keeping already-renamed files applied). Cross-fixer same-file edits compose: the second fixer reads the first fixer's pending staged content (not the original disk bytes) so unused-export removal AND unused-enum removal on the same source file land coherently in one rewrite instead of overwriting each other via last-write-wins. The commit path follows symlinks: the canonicalized resolved path drives the final rename, so a symbolic link to the real source file is preserved post-fix and the edit flows through to the real target. The JSON envelope gains a top-level `skipped_content_changed: number` (always present) and a new per-file entry shape `{type: "skipped", path: ..., skipped: true, skip_reason: "content_changed"}` in the `fixes[]` array; the existing `skipped` counter is unchanged (it keeps its prior meaning of catalog / YAML guard skips only). The action's `summary-fix.jq` template counts successful fix attempts in its headline (excluding skip entries) and surfaces `skipped_content_changed` in the prose. Hash precondition covers source files (TS / JS / Vue / Svelte / Astro / MDX); `package.json` and `pnpm-workspace.yaml` are not in the captured hash map because the extract layer does not parse them, and the dep / catalog fixers re-parse those files at fix time as the natural safety net. Skipped files contribute a non-zero exit code, so CI surfaces the diagnostic instead of treating a mid-flight edit as a successful no-op. Five new integration / unit tests cover round-trip clearing of targeted findings, batch abort when one target directory is read-only, the envelope-field contract, cross-fixer composition, and symlink follow-through. (Closes [#454](https://github.com/fallow-rs/fallow/issues/454).)

- **GitHub Action and GitLab CI templates surface API failures and pagination errors instead of running unscoped analysis or posting duplicate PR/MR comments.** Two silent-fail patterns are closed. (1) `action/scripts/analyze.sh` previously masked `gh api --paginate` failures behind `2>/dev/null || true`, so a transient 500, expired token, or missing repo permissions silently fell through to the existing "shallow clone" warning. The action then ran analysis against the full codebase and posted every finding, not just findings on PR-changed lines. The new path captures gh stderr and exit code separately, emits a `::warning::` naming the API failure mode, and writes `changed_files_unavailable=true` to `$GITHUB_OUTPUT`. The marker is initialized to `false` unconditionally at the top of analyze.sh so downstream `if:` gates always see a definitive value rather than an absent-vs-false ambiguity. (2) Six call sites in `action/scripts/{review,comment}.sh` and `ci/scripts/{review,comment}.sh` consumed paginated API results via `paginate_fn ... | jq ... || echo '[]'` and `... | head -1 || true` patterns. When pagination failed mid-stream (the canonical case: GitLab API on a >100-comment MR returning a 502 on page 2 after page 1 succeeded), the empty fallback fed the dedup logic and produced N duplicate inline threads on every retry. Each call site now consumes the helper's exit code via a two-step `if paginate_fn > tmp; then ...; else warning + marker + exit; fi` pattern. Failure semantics diverge by surface: the multi-comment fingerprint dedup path aborts the post step (no inline review POST) because N duplicate threads is materially worse than no inline review; the summary-only path posts a fresh comment anyway (a duplicate summary is annoying but collapsible, while a missing summary is silently broken). Exit code on the abort path is conditional on the failure class: stderr matching `HTTP 4[0-9][0-9]` or `error: 4[0-9][0-9]` (and NOT `HTTP 429` / `error: 429` / `rate.limit`) escalates to `exit 1` for loud CI failure because a re-run will not resolve a configuration error; everything else (5xx, retry-exhausted 429, network errors) falls through to `exit 0` so transient blips do not break PRs. The action exposes three new composite outputs for downstream workflow gates: `changed-files-unavailable` (always emitted from analyze.sh), `post-skipped-reason` (`none` / `pagination_failure`, set to `pagination_failure` only on the multi-comment abort), and `dedup-lookup-failed` (`true` whenever a dedup lookup failed, including the summary-only path where the post proceeds). The GitLab template writes parallel sidecar artifacts (`fallow-skip-reason.txt`, `fallow-dedup-lookup-failed.txt`) declared in `artifacts: paths:` so downstream jobs can gate on them. `gitlab-ci.yml`'s existing `bash review.sh || echo "WARNING: ..."` swallows our `exit 1`, so the 4xx escalation is advisory for the default template; operators who want CI-fatal behavior on auth misconfiguration can drop the `|| echo` from their template or gate on the sidecar markers. `mktemp` files are cleaned up via a single `trap '... EXIT'` per script rather than inline `rm -f` so signal-driven exits do not leak tmp files. Regression tests inject synthetic 5xx, 4xx, and retry-exhausted 429 failures on every affected call site and assert the marker contracts plus the absence of duplicate POSTs. (Closes [#470](https://github.com/fallow-rs/fallow/issues/470).)

- **Re-export chain resolution propagates references through barrel chains of arbitrary depth and tags multi-hop `export type *` synthetic stubs as type-only.** Before, the propagation fixpoint capped at 20 iterations and emitted only an invisible `tracing::warn!` when a chain truncated, so barrels deeper than 20 hops silently under-credited references on the leaf. After, the fixpoint terminates naturally via the existing dedup-by-`from_file` check, with a defensive safety backstop bounded by the number of re-export edges. Re-export cycles (`a.ts: export * from './b'` paired with `b.ts: export * from './a'`) and barrels that re-export from themselves are now surfaced as one `tracing::warn!` per cycle / self-loop with the member file paths, so they are discoverable when debugging. The synthetic bridge stub created on the source module during multi-hop star propagation inherits `is_type_only` from the triggering re-export edge instead of hardcoding `false`; two star edges reaching the same source with conflicting flags resolve to the value-bearing variant via a dedicated downgrade pass. Users running `--save-baseline` on barrel-heavy projects with chains longer than 20 hops may see slightly fewer findings after upgrade because the deeper hops now propagate correctly. Surfacing the cycle diagnostic as a structured `AnalysisResults` finding is tracked in [#515](https://github.com/fallow-rs/fallow/issues/515). (Closes [#442](https://github.com/fallow-rs/fallow/issues/442).)

- **`unused-class-members` no longer false-positives when a Playwright fixture is created with `base.extend<MyFixtures>(...)` and returned from a helper function.** The #268 fix credited POM methods consumed via a directly-bound `const test = base.extend<MyFixtures>(...)`, but the helper form (`export function appTest() { return base.extend<MyFixtures>(...); }` consumed as `appTest()(...)`) reintroduced the false positive on nested POM methods. Three declarator shapes now capture the helper (function declaration, `const = () => ...`, `const = function () { ... }`) when the body is a single `return <call>` statement; a finalize-time pass gates the `base` local on `@playwright/test`'s `test` named import and emits the same def-sentinel `MemberAccess` the analyzer already correlates with use sentinels. Same-file helper chains (`function appTest() { return setupTestFixture(); } function setupTestFixture() { return base.extend<T>(...); }`) propagate via a capped fixed-point pass over `(caller, callee)` aliases captured at the same site. The use-side `appTest()(...)` form is recognised by extending `playwright_test_callee_name` to recurse into a `CallExpression` callee. Cross-file helper chains are intentionally out of scope. The extraction cache (`CACHE_VERSION` bumped to 86) invalidates automatically on upgrade so users with warm `.fallow/cache` directories get the fix without manual clearing. Thanks [@vethman](https://github.com/vethman) for the report. (Closes [#491](https://github.com/fallow-rs/fallow/issues/491).)

- **Monorepo workspace discovery surfaces malformed `package.json`, unreachable glob matches, and missing tsconfig references instead of silently dropping them.** Seven silent-drop sites in `crates/config/src/workspace/{mod.rs,parsers.rs}` are replaced with typed `WorkspaceDiagnostic` values carrying a kind discriminator (`malformed-package-json`, `glob-matched-no-package-json`, `malformed-tsconfig`, `tsconfig-reference-dir-missing`, plus the existing `undeclared-workspace`). Severity is per-site: a malformed ROOT `package.json` exits 2 at config load (mirrors `validate_resolved_boundaries` from #468) because no workspace patterns can be collected without a parseable root; a malformed declared-workspace `package.json` (or tsconfig-referenced `package.json`) warns and continues so analysis still runs on the remaining workspaces; a glob match that resolves to a directory without `package.json` warns only when the path is NOT in the conventional skip list (extended to `dist`, `coverage`, plus dot-prefixed `.cache` / `.next` / `.turbo` / `.nuxt` / `.svelte-kit` via the existing `starts_with('.')` arm) AND NOT covered by user `ignorePatterns`; a shallow-scan candidate `package.json` stays silent because the user did not declare it and the heuristic should not generate noise; a missing `tsconfig.json` stays silent because many JS-only projects have none; a malformed `tsconfig.json` warns. Diagnostics emit via deduplicated `tracing::warn!` (process-wide `OnceLock<Mutex<FxHashSet>>` keyed on canonical-root + kind + path, matching the staged migration pattern of #467) and also land in a process-wide registry keyed by canonical root so the JSON envelope on `fallow check`, `fallow dupes`, and `fallow health` carries a new optional `workspace_diagnostics: WorkspaceDiagnostic[]` field. The field is omitted when empty (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`), so consumers on monorepos without discovery noise see no wire change. A new `fallow list --workspaces` flag (alias: `fallow workspaces`) renders the discovered workspace table plus the diagnostics section; on a single-package project the explicit form prints `No workspaces declared (single-package project).` instead of silence. The `find_undeclared_workspaces_with_ignores` pass now folds its results into the same registry and suppresses entries that already carry a `MalformedPackageJson` diagnostic so a typo'd `package.json` is not flagged twice (once as malformed, once as "undeclared"). Every warning message ends with a concrete next step ("Fix the JSON syntax", "remove from the workspaces pattern", "add to `ignorePatterns`") so first-time users have a path forward. (Closes [#473](https://github.com/fallow-rs/fallow/issues/473).)

- **Plugin system silent-fail patterns now surface as `tracing::warn!` diagnostics at config load instead of hiding behind broken configuration.** Three patterns in `crates/core/src/plugins/registry/` join the family already addressed by #449 / #457 / #467 / #468 / #473. (1) When two plugins (built-in OR external) declare a byte-identical `config_patterns` entry, fallow emits `plugin config_patterns collision: identical pattern 'pat' is claimed by plugins [a, b]; 'a' runs first (registration order), others (b) follow.` once per (pattern, owners) tuple, naming both sides and documenting the registration-order precedence rule (the owner that registered first wins Phase 3a matching). Detection is byte-equal on the pattern string; sub-glob overlap (e.g. `vite.config.{ts,js}` vs `vite.config.ts`) requires pattern-intersection logic that is not in `globset` and is intentionally deferred to a follow-up issue. (2) An external plugin (file-loaded from `.fallow/plugins/*.{toml,json,jsonc}`, root `fallow-plugin-*`, or inline `framework[]` in the config) whose `enablers` does not match any project dependency AND whose closest dep name is within Levenshtein distance two now warns `plugin 'my-vue' enabler '@vue/cor' does not match any dependency in package.json; did you mean '@vue/core'?`. Plugins with a richer `detection` block or empty `enablers` are skipped. No warning fires when no Levenshtein-close dep exists, so a plugin that legitimately does not apply to this project stays silent. The Levenshtein helper is lifted into `crates/config/src/levenshtein.rs` so both the rule-name path (`closest_known_rule_name` from #467) and the new enabler-typo path share one distance/length policy. (3) Invalid regexes in `PathRule.exclude_regexes` / `exclude_segment_regexes` (extracted by built-in plugins from user code such as TanStack Router route options) now validate eagerly inside `process_config_result` instead of failing silently at matcher use-time. Invalid patterns are dropped after a `plugin 'tanstack-router' in /path/to/routes.ts: invalid excluded regex '[unclosed' for entry pattern 'src/routes/...': ...; the pattern will be ignored.` warning. All three diagnostics dedupe process-wide so combined-mode (`check` + `dupes` + `health` through one config load path) emits at most one warning per (kind, key) pair. Tracing-warn matches the staged migration pattern of #467 / #510: every warning closes with "A future release may reject ..." so users know a hard-error escalation is on the roadmap once false-positive rates are well-understood. (Closes [#479](https://github.com/fallow-rs/fallow/issues/479).)
- **Misspelled rule names in `.fallowrc.json` / `fallow.toml` now warn at config load instead of silently falling back to the default severity.** `crates/config/src/config/rules.rs` ships a canonical `KNOWN_RULE_NAMES` list covering every canonical kebab-case rule name plus every documented `#[serde(alias)]` form, and a Levenshtein-distance helper that suggests the closest match when an unknown key is plausibly a typo. `FallowConfig::load` walks the merged config value after `extends` resolution and emits one `tracing::warn!` per unknown key under `rules` or `overrides[i].rules`, deduped process-wide so combined-mode runs (`check` + `dupes` + `health` through one config load path) emit at most one warning per (context, key) pair. A `.fallowrc.json` with `{"rules": {"unsued-files": "warn"}}` now logs `unknown rule 'unsued-files' in rules (did you mean 'unused-files'?); the rule will be ignored. A future release will reject unknown rule names.` instead of silently leaving `unused-files` at its default severity. Two drift guards in the test suite catch future field or alias additions that forget to update the list. Phase 1 of a staged migration: a future minor release will flip `RulesConfig` and `PartialRulesConfig` to `#[serde(deny_unknown_fields)]` and turn the warning into a hard config-load error. (Closes [#467](https://github.com/fallow-rs/fallow/issues/467).)
- **LSP hover content embeds user-controlled identifiers via CommonMark code spans, and the `remove unused export` quick fix validates the live declaration shape before producing an edit.** Two LSP fragilities are closed. (1) Hover bodies in `crates/lsp/src/hover.rs` previously built `MarkupKind::Markdown` content via bare `format!()` with no escaping of interpolated export names, member names, import specifiers, or duplication file paths. A crafted identifier such as `` `[click](command:vscode.open?evil)` `` would render as a clickable link in any LSP client that promotes hover content to trusted MarkdownString. VS Code's default `vscode-languageclient` configuration treats hover content as untrusted, so `command:` URIs do NOT execute under default settings; the realised risk is link-on-hover (image fetch to attacker-controlled domain, link-to-phishing on click) plus corrupted rendering. The new `format_inline_code(value)` helper in `crates/lsp/src/markdown.rs` wraps each interpolated value in a CommonMark inline code span with a backtick-fence length one greater than the longest backtick run in the value (with space padding when the value starts or ends with a backtick), so the value renders verbatim and cannot break out of the span even when it contains backticks. Inside a code span CommonMark suppresses every inline construct, neutralising every link-style injection vector. The helper is portable across VS Code, Helix, Zed, and Neovim. (2) `build_remove_export_actions` in `crates/lsp/src/code_actions/quick_fix.rs` previously trusted the cached `export_line` from `AnalysisResults` (computed at `did_save` time). A `did_change` between `did_save` and the `code_action` request would leave the cached line pointing at text the user had reshaped in memory, and the action would strip a leading `export ` prefix from an unrelated declaration. The handler now validates the live line via `declares_export_name`, which strips `export ` and any declaration / modifier keywords (`const`, `let`, `var`, `function`, `function*`, `class`, `type`, `interface`, `enum`, `namespace` plus `async`, `abstract`, `declare`) and asserts the leading identifier equals the cached name. The check rejects substring collisions (cached `foo` vs live `foobar`), value-position collisions (cached `foo` vs live `export const bar = foo;` where `foo` is referenced as a value, not the declared name, which would otherwise silently strip `export ` from `bar`), and re-export blocks (whose edit would produce a syntax error). `export default` declarations stay on prefix-only validation because each file has at most one default export and the cached finding refers to "the default export of this file" regardless of its current shape. `Diagnostic.message` synthesised in code-action handlers stays plain text per the LSP specification, matching the published diagnostic in `crates/lsp/src/diagnostics/unused.rs` so VS Code's "Fix all in file" correlation keeps working. (Closes [#480](https://github.com/fallow-rs/fallow/issues/480).)

- **`fallow audit` worktree lifecycle is hardened against panic-cleanup leaks, Windows orphan accumulation, and parallel-CI cache races.** Three structural fragilities in `crates/cli/src/audit.rs` are fixed in one pass: (1) a hand-rolled `WorktreeCleanupGuard` is armed before every `git worktree add` subprocess and disarmed only after the `BaseWorktree` struct is constructed, so any early-return path between subprocess success and struct construction now rolls back BOTH git's `.git/worktrees/<name>` registration AND the on-disk directory; (2) `process_is_alive` on Windows was a `#[cfg(not(unix))] true` stub, so `sweep_orphan_audit_worktrees` never cleaned worktrees owned by dead PIDs on Windows. It now calls `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION) + WaitForSingleObject(handle, 0)` via target-gated `windows-sys` bindings, distinguishing `ERROR_INVALID_PARAMETER` (dead) from `ERROR_ACCESS_DENIED` (conservative: alive, matches `kill -0` semantics under EPERM). Long-running Windows dev machines and CI runners stop leaking disk across runs; (3) `BaseWorktree::reuse_or_create` now takes a kernel-level advisory lock on `<reusable_audit_worktree_path>.lock` for the duration of its critical section, via `std::fs::File::try_lock` (stable since Rust 1.89, wraps `flock(2)` on Unix and `LockFileEx` on Windows). Concurrent `fallow audit` runs against the same `base_sha` in matrix CI no longer race on `git worktree add` against the same cached path; the loser cleanly falls through to the non-reusable PID-named worktree instead of racing. `remove_audit_worktree` additionally emits a `tracing::warn!` when `git worktree remove --force` returns non-zero AND the on-disk directory survives, so leaked git registrations are observable via `RUST_LOG=warn` rather than silent. No new third-party deps; `windows-sys` is target-gated to `cfg(windows)`. (Closes [#472](https://github.com/fallow-rs/fallow/issues/472).)
- **Barrel re-export member propagation through `extends` / `implements` is no longer order-sensitive.** Class-inheritance and interface-implementer credit now walks re-export origins, so a child class `extends BaseShape` (or `implements RenderableShape`) through a `contracts/index.ts` barrel correctly credits the parent's `this.*` accesses on the actual defining file. The visitor also now resolves `export { X }; import { X } from './a';` (export-before-import) as a re-export instead of a local export, matching the existing import-before-export behavior. A local declaration of `X` in the same module shadows the import and keeps the export local. The extraction cache is invalidated automatically on upgrade so users with warm `.fallow/cache` get the fix without manual clearing. Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza) for the patch. (Closes [#427](https://github.com/fallow-rs/fallow/pull/427).)

- **Mask `api_key` and license JWT in `Debug` output and sanitize `Bearer` tokens before they reach stderr.** `CloudRequest` and `UploadInventoryArgs` previously derived `Debug` on their public `api_key` field, and `ActivateArgs` derived `Debug` on its raw-JWT field; any future `tracing::debug!`, `dbg!`, or unwrap-on-Err with the `Debug` formatter would have emitted the full credential to stderr. The derives are replaced with manual `impl Debug` that render the secret as `"***"` (or `Some("***")` for the optional cases). A new `sanitize_network_error(detail: &str)` helper in `crates/cli/src/api.rs` walks user-facing network-error strings and replaces every `Bearer <token>` substring with `Bearer ***`. The helper is wired at every `format!("{err}")` site that runs after an `Authorization: Bearer ...` header is set: `coverage/cloud_client`, `coverage/upload_inventory`, `coverage/upload_source_maps`, `license::activate_trial`, `license::refresh_active_license`, and `ci::with_rate_limit_retry` (GitHub PR-comment posting). The token charset matches the JWT / fallow API-key alphabet (`A-Za-z0-9_.\-=`); non-token bytes after `Bearer ` are preserved so prose mentioning the literal `Bearer ` is untouched. No real disclosure has hit production; this is a defensive lockdown against `ureq::Error`'s `Display` impl that can include outgoing request headers on TLS / connection / internal failure modes. (Closes [#476](https://github.com/fallow-rs/fallow/issues/476).)

## [2.76.0] - 2026-05-19

### Deprecated

- **`fallow_core` is now an internal-only surface per [ADR-008](decisions/008-fallow-core-internal-policy.md).** The top-level entry points (`analyze`, `analyze_with_usages`, `analyze_with_trace`, `analyze_retaining_modules`, `analyze_with_parse_result`, `analyze_project`), the `fallow_core::analyze::*` detector helpers (`find_dead_code_full`, `find_unused_exports`, `find_duplicate_exports`, `find_unused_files`, `find_unused_members`, `find_unused_dependencies`, `find_unused_dependency_overrides`, `find_misconfigured_dependency_overrides`, `find_unused_catalog_entries`, `find_empty_catalog_groups`, `find_unresolved_catalog_references`, `find_boundary_violations`), and the feature-flag helpers (`collect_feature_flags`, `correlate_with_dead_code`) now carry `#[deprecated(since = "2.76.0")]` annotations pointing external consumers at `fallow_cli::programmatic` (or `fallow flags --format json` for feature-flag data). Workspace path-dependency callers in `fallow-cli`, `fallow-lsp`, `fallow-core` itself, and the in-tree integration tests continue to compile via `#[expect(deprecated)]` at the call sites. The next minor release (target `2.77.0`, no earlier than 2026-Q3) will flip `publish = false` on `fallow-core` so the crate is no longer fetchable from crates.io; this release is the one-cycle warning window for any external consumers. The replacement programmatic functions return `serde_json::Value` matching the CLI's `--format json` contract (with `*Finding` wrappers, so e.g. `unused_exports[i].export.path` rather than `unused_exports[i].path`), and structured `ProgrammaticError { exit_code, ... }` mirrors the CLI's exit-code ladder so CI integrations branching on exit codes work identically. See [`docs/fallow-core-migration.md`](docs/fallow-core-migration.md) for the function-by-function map and a minimal usage example. Consumers running `RUSTFLAGS=-Dwarnings` will need to allow `deprecated` on their `fallow-core` call sites or migrate before upgrading. (Closes [#418](https://github.com/fallow-rs/fallow/issues/418).)

### Changed

- **`--diff-file <PATH>` now scopes every finding to changed lines, not just the runtime-coverage `hot-path-touched` verdict.** Before: a developer running `fallow audit --diff-file pr.diff` saw line-level filtering only on the runtime-coverage section; every other finding type (unused-export, complexity hotspot, clone family, boundary violation, etc.) stayed at full project scope, and the GitHub Action / GitLab CI shipped a downstream jq filter (`filter-changed.jq`) to narrow the JSON output before posting to PRs. After: when `--diff-file` (or `$FALLOW_DIFF_FILE`) is set, fallow narrows ALL source-anchored findings to lines inside an added hunk for that file, so the JSON `total_issues` is already accurate without jq post-processing. Project-level findings (unused-deps, catalog entries, dependency overrides) bypass the filter because they anchor at fixed `package.json` / `pnpm-workspace.yaml` lines a PR rarely touches even when it semantically caused the finding (e.g. removing the last consumer of `lodash`). Range-shaped findings filter via overlap: a complexity hotspot at lines 10..120 touched by a PR at line 115 is kept; a clone family is kept when ANY of its instances overlaps the diff, even when the other instances are off-diff. Pass `-` as the path or use the new `--diff-stdin` alias to read the diff from stdin (`gh pr diff | fallow audit --diff-file -`). When both `--diff-file` and `--changed-since` are set, `--diff-file` wins for line-level filtering and `--changed-since` still scopes file discovery; fallow logs a one-line stderr note so the precedence is visible in CI logs. An opt-in `--diff-file` that parses 0 added lines (typo, wrong path, pure-rename diff with no content hunks) now emits a `fallow: warning [diff-file]: ... parsed 0 added lines` warning instead of silently dropping every finding. Existing Action / CI pipelines that already set `FALLOW_DIFF_FILE` keep working unchanged with stricter filtering; pipelines that relied on the pre-feature behavior (full project scope when `--diff-file` was set for runtime-coverage only) should unset `FALLOW_DIFF_FILE` or remove the flag. (Closes [#424](https://github.com/fallow-rs/fallow/issues/424).)

- **`fallow fix` can remove leading pnpm-catalog comment blocks when deleting unused catalog entries.** The new `fix.catalog.deletePrecedingComments` config knob accepts `"auto"` (default), `"always"`, or `"never"`. `auto` deletes a contiguous same-indent YAML comment block only when it directly follows the parent catalog header or a blank separator, preserving sibling-adjacent comments for manual review. Two escape hatches keep curated comments safe: a `# fallow-keep` marker on any line in the block preserves it regardless of policy (mirroring the existing `fallow-ignore` inline-suppression convention), and the `auto` policy additionally preserves section-banner blocks whose comment body starts with three or more `=`, `-`, `*`, `_`, `~`, `+`, or `#` characters (e.g. `# === React 18 production pins ===`). The JSON fix action gains an `entry_line` field alongside the (now deletion-start) `line` field so CI annotators and agents can pick whichever anchor they need without ambiguity; the human-mode summary surfaces the absorbed comment-line count (`Fixed N issue(s) (+M catalog comment lines)`). LSP quick fixes intentionally stay conservative and do not delete leading comments. (Closes [#360](https://github.com/fallow-rs/fallow/issues/360).)

- **Duplication `actions` flow through serde natively; the `inject_dupes_actions` JSON post-pass is fully retired.** `fallow dupes --format json` (and the `dupes` block of `audit` / combined output, and the per-bucket output under `--group-by`) now serialises each `clone_groups[]` item through a typed `CloneGroupFinding` wrapper that flattens the bare `CloneGroup` payload and carries `actions: CloneGroupAction[]` plus the optional audit-mode `introduced: boolean` as named fields. Each `clone_families[]` item flows through a typed `CloneFamilyFinding` wrapper; its nested `groups[]` items each carry their own typed `actions[]` array, preserving the issue [#393](https://github.com/fallow-rs/fallow/issues/393) regression contract for JSON-Schema strict consumers. Per-bucket `--group-by` items flow through `AttributedCloneGroupFinding`. The on-the-wire shape is unchanged: every inner field (`instances`, `token_count`, `line_count`, `files`, `groups`, `total_duplicated_lines`, `total_duplicated_tokens`, `suggestions`, `primary_owner`, `instances[].owner`) still sits at the top level of each item alongside `actions` and (top-level only) `introduced`. The OLD-vs-NEW JSON diff on the vue-core benchmark fixture is byte-identical modulo `elapsed_ms` / `version` for both standalone `dupes --format json` and the bare combined invocation. A new `DupesReportPayload` mirrors `DuplicationReport` field-for-field with wrapped findings, and `DupesOutput.report` / `CombinedOutput.dupes` / `AuditOutput.duplication` now flatten the payload instead of the bare report. The 120-line `inject_dupes_actions` post-pass plus its `build_clone_family_actions` / `build_clone_group_actions` helpers are deleted from `crates/cli/src/report/json.rs`. With this batch the legacy `inject_*_actions` post-pass is fully retired across check, dupes, and health. The codegen-derived TS contracts (`editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts`) gain matching `CloneGroupFinding` / `CloneFamilyFinding` / `AttributedCloneGroupFinding` interfaces; backwards-compat aliases (`type CloneGroup = CloneGroupFinding`, `type CloneFamily = CloneFamilyFinding`, `type AttributedCloneGroup = AttributedCloneGroupFinding`) live in `editors/vscode/src/types.ts` so existing consumers see no API break. (Closes [#409](https://github.com/fallow-rs/fallow/issues/409). Refs [#384](https://github.com/fallow-rs/fallow/issues/384).)

- **`fallow coverage analyze --format json` envelope is now derived from a typed Rust struct; the last hand-built `serde_json::json!` root envelope is retired.** `print_runtime_json` in `crates/cli/src/coverage/analyze.rs` previously built the output via a `serde_json::json!({"schema_version": "1", "version": ..., "elapsed_ms": ..., "runtime_coverage": ...})` macro and grafted the optional `_meta` block via a JSON-map insertion. The envelope now flows through a typed `CoverageAnalyzeOutput` struct (singleton `schema_version: CoverageAnalyzeSchemaVersion::V1` discriminator, plus `version`, `elapsed_ms`, `runtime_coverage: RuntimeCoverageReport`, optional `_meta: Meta`) so the schema is derived from Rust source. `CoverageAnalyzeOutput` joins `FallowOutput` as a new untagged `CoverageAnalyze` variant rather than staying a hand-maintained sibling root `oneOf` branch; the document-root `oneOf` simplifies to two entries (`FallowOutput` + `CodeClimateOutput`). The wire shape is byte-identical: same four required fields, same optional `_meta`. The drift-gate registry in `crates/cli/src/bin/schema_emit.rs` drops the `CoverageAnalyzeOutput` entry from both `HAND_MAINTAINED_ALLOW_LIST` and `HAND_MAINTAINED_ROOT_ENVELOPES`, both now empty. The root schema `description` field gains `runtime_coverage` as a discriminator and drops the "still hand-maintained" caveat. (Closes [#410](https://github.com/fallow-rs/fallow/issues/410). Refs [#384](https://github.com/fallow-rs/fallow/issues/384).)

- **Hotspot and refactoring-target `actions` flow through serde natively; the JSON post-pass that patched them in is fully retired.** `fallow health --hotspots --targets --format json` (and the `health` block of `audit` / combined output, and the per-group output under `--group-by`) now serialises each `hotspots[]` item through a typed `HotspotFinding` wrapper and each `targets[]` item through a typed `RefactoringTargetFinding` wrapper. Both wrappers flatten the inner payload via `#[serde(flatten)]` and carry `actions` as a named field (`HotspotAction[]` and `RefactoringTargetAction[]` respectively). The on-the-wire shape is unchanged: every inner field (`HotspotEntry`'s `path`, `score`, `commits`, `weighted_commits`, `lines_added`, `lines_deleted`, `complexity_density`, `fan_in`, `trend`, `ownership`, `is_test_path`; `RefactoringTarget`'s `path`, `priority`, `efficiency`, `recommendation`, `category`, `effort`, `confidence`, `factors`, `evidence`) still sits at the top level alongside `actions`. The OLD-vs-NEW JSON diff on the vue-core benchmark fixture is byte-identical modulo `elapsed_ms` / `version` for `health --hotspots --targets`, the bare combined invocation, and `audit --base HEAD~1`. The schema ships separate `HotspotFinding` / `HotspotEntry` and `RefactoringTargetFinding` / `RefactoringTarget` definitions in `docs/output-schema.json`, and the codegen-derived TS contracts (`editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts`) gain matching `HotspotFinding` / `RefactoringTargetFinding` interfaces plus `HotspotEntry = Omit<HotspotFinding, "actions">` and `RefactoringTarget = Omit<RefactoringTargetFinding, "actions">` type aliases so downstream consumers can type either the wrapper or the inner payload. Asymmetry with `HealthFinding`: neither `HotspotFinding` nor `RefactoringTargetFinding` carries the audit-mode `introduced` flag because hotspot ranking and refactoring targets are not produced by `fallow audit`'s base-snapshot classifier; the pre-wrapper `finding_augmentation` set `include_introduced: false` for both and the typed wrappers preserve that. With this batch the legacy `inject_health_post_pass_actions` post-pass is fully retired across `report/json.rs`, `audit.rs`, and `combined.rs`. (Closes [#408](https://github.com/fallow-rs/fallow/issues/408). Refs [#384](https://github.com/fallow-rs/fallow/issues/384).)

- **Health-finding `actions` and `introduced` flow through serde natively; the JSON post-pass that patched them in is retired for findings.** `fallow health --format json` (and the `health` block of `audit` / combined output) now serialises each `findings[]` item through a typed `HealthFinding` wrapper that flattens the inner `ComplexityViolation` payload and carries `actions: HealthFindingAction[]` plus the optional audit-mode `introduced: boolean` as named fields. The on-the-wire shape is unchanged: every inner field (`path`, `name`, `line`, `col`, `cyclomatic`, `cognitive`, ...) still sits at the top level of each `findings[]` item alongside `actions` and `introduced`. The OLD-vs-NEW JSON diff on the vue-core benchmark fixture (273 findings) is empty modulo `elapsed_ms` / `version`, and per-finding `actions[]` discriminants are byte-identical. The schema now ships separate `HealthFinding` and `ComplexityViolation` definitions in `docs/output-schema.json` (`HealthFinding` describes the wire envelope; `ComplexityViolation` is the inner payload); the codegen-derived TS contracts (`editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts`) gain a matching `HealthFinding` interface plus a `ComplexityViolation` type alias (`Omit<HealthFinding, "actions" | "introduced">`) so downstream consumers can type either the wrapper or the inner payload. The `inject_health_actions` post-pass that previously mutated the serialised JSON tree to add `actions` arrays and the report-root `actions_meta` breadcrumb is retired for findings; the renamed `inject_health_post_pass_actions` still walks `targets[]` and `hotspots[]` until their typed-wrapper migrations land (out of scope for this PR). `actions_meta` is now set at construction time on the typed `HealthReport.actions_meta` field (and on each per-group `HealthGroup.actions_meta` when `--group-by` is active) when the active `HealthActionContext` requests suppress-line omission, so the breadcrumb flows through serde natively. (Refs [#384](https://github.com/fallow-rs/fallow/issues/384).)

### Added

- **Angular component complexity rollup: synthesise a `<component>` finding per component that has both class AND template above threshold.** `fallow health --complexity` (and `audit`, `combined`) now emits a synthetic `<component>` finding per Angular component whose class function findings AND `<template>` finding both cleared the cyclomatic / cognitive thresholds. The rollup's `cyclomatic` and `cognitive` are `worst_class_method + template`; the new `component_rollup` payload carries the pre-summation breakdown so consumers see WHY the component ranks high (`class_worst_function: "handleClick"`, `class_cyclomatic: 3`, `class_cognitive: 4`, `template_path: "src/host-game.component.html"`, `template_cyclomatic: 6`, `template_cognitive: 9`, `component: "host-game.component"` derived from the .ts owner's file stem). A component whose class scores moderately (3/4) and whose template scores moderately (6/9) was previously two scattered medium findings the headline rank could miss; the rollup surfaces it as one finding cyclomatic 9, cognitive 13 that wins `--targets` selection. Per-function and per-`<template>` entries stay where they are so existing suppression sites and per-finding tooling references keep working: the rollup is strictly additive. The rollup is anchored at the worst class method's line so `// fallow-ignore-next-line complexity` placed above that method hides both the function finding and the rollup; the JSON action declares `placement: "above-component-worst-method"`. Both external-`templateUrl` components (using the inverse-`templateUrl` provenance map populated alongside CRAP scoring) and inline `template:` components (where the `<template>` finding already lives on the owning .ts) are handled; defensive against the rare multi-`@Component` .ts file (no rollup, since fallow can't yet AST-attribute each template to a specific class). Wire-level deltas: (a) new `component_rollup: ComponentRollup` optional field on every `HealthFinding`, gated by `#[serde(default, skip_serializing_if = "Option::is_none")]` so non-rollup findings see no shape change; (b) new `ComponentRollup` definition in `docs/output-schema.json` and the codegen-derived TS contracts (`editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts`); (c) `fallow health` human output renders a `rolled up: 3cyc 4cog on host-game.component.handleClick + 6cyc 9cog on host-game.component.html` breakdown line under the `<component>` row; (d) the MCP `check_health` tool description teaches AI agents how to read the discriminator and the breakdown so the rollup steers refactoring decisions. Existing baselines see the `<component>` findings as new (`audit --gate new-only` will surface them); rebake baselines after upgrade if you want to suppress the rollup historically. (Closes [#234](https://github.com/fallow-rs/fallow/issues/234).)

- **Synthetic Angular `<template>` complexity findings now carry component-inherited CRAP provenance.** When `fallow health --complexity` (and `audit`, `combined`) emit a `<template>` finding on a `.html` template, the CRAP score is computed against the owning `.component.ts` reached via the inverse `templateUrl` `SideEffect` edge, and the finding ships two new fields: `coverage_source: "estimated_component_inherited"` and `inherited_from: "<owner>.component.ts"` (project-relative). The score itself does not move for the common case (BFS already cascades reachability through the `templateUrl` edge, so `.html.is_test_reachable()` already matches the owning `.ts`), so existing baselines and `--max-crap` gates are unaffected. The wire-level deltas are: (a) the two new optional fields appear on `HealthFinding` when the inherit path triggered, gated by `#[serde(skip_serializing_if = "Option::is_none")]` so existing consumers see no shape change otherwise; (b) the new `CoverageSource` enum (`istanbul` / `estimated` / `estimated_component_inherited`) is published in `docs/output-schema.json` and the codegen-derived TS contracts (`editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts`); (c) `fallow health` human output renders `(inherited from foo.component.ts)` after the CRAP score on template rows; (d) the JSON action ladder pivots the primary coverage action for inherited `<template>` findings from "increase coverage on `<template>`" to "increase test coverage on `<owner>.component.ts`" with a `target_path` field, so AI agents target the component rather than scaffolding tests against a structurally untestable `.html` path. The MCP `check_health` tool description now explains the discriminator so agents discover the new fields without inspecting the schema. Tier 2 (AOT source-map back-mapping for projects whose tests are AOT-compiled) is tracked separately on the same issue. (Refs [#186](https://github.com/fallow-rs/fallow/issues/186) tier 1.)

- **`fallow fix` creates `.fallowrc.json` when no fallow config exists, with framework-aware scaffolding plus the new `ignoreExports` rules.** Previously, `fallow fix` against a project with `duplicate-exports` findings and no fallow config file printed `Skipped duplicate-export config fix: no fallow config file at <root>. Run fallow init to create one, then re-run fallow fix --yes.` and exited without writing. Users had to run two commands to fix what felt like one issue. The applier now creates `.fallowrc.json` directly using the same scaffolding `fallow init` emits (framework detection for TypeScript / Storybook / Vitest / Jest / Playwright / React / Vue / Angular / Svelte, `$schema`, `entry`, `ignorePatterns`, `duplicates.minOccurrences = 3`, `rules.unused-dependencies = "warn"`) and layers the new `ignoreExports` entries on top, so the post-fix `fallow check` does not surface a fresh wave of `unused_files` for Storybook files or `unused_dependencies` for test-framework packages. The create-fallback refuses to fire inside a monorepo subpackage (one of `pnpm-workspace.yaml`, `package.json#workspaces`, `turbo.json`, `lerna.json`, `rush.json` above the invocation directory) and emits a targeted error pointing at the workspace root instead, so 8-package monorepos do not end up with 8 fragmented configs. A new `--no-create-config` flag (default off) opts out of the create-fallback for pre-commit hooks, CI bots, and `fallow watch` where silently materialising a new top-level file would surprise the user; the duplicate-export config-add path is skipped with an explanatory entry while source-file edits proceed normally. After a successful create, the human stderr emits `Created .fallowrc.json with N ignoreExports rule(s). Add it to git.` so the new artifact is not silently lost. The JSON `add-to-config` action's `auto_fixable` field is now `true` whenever the applier can safely write the rule (config exists, OR no config exists and the working directory is not inside a monorepo subpackage); the per-instance flip remains for the monorepo case, so agents that filter on `auto_fixable: true` continue to receive only actions they can safely pipe into `fallow fix --yes`. (Closes [#332](https://github.com/fallow-rs/fallow/issues/332).)

- **`fallow fix --dry-run` previews `ignoreExports` config writes as a unified diff, replacing the prior `Would add N rule(s)` count-only line.** Both human (`--format human`) and JSON (`--format json`) modes carry the proposal in full. For the create-fallback case the diff is hand-rolled `+`-prefix output (BEFORE side is always empty). For the edit case it is a `similar::TextDiff` unified diff with 3 lines of context so callers can audit the exact bytes that will land in `.fallowrc.json` / `.fallowrc.jsonc` / `fallow.toml` / `.fallow.toml`. The JSON fix entry gains a `proposed_diff: "..."` field alongside the existing `entries: [...]` breadcrumb so agents piping `fallow fix --dry-run --format json` into `fallow fix --yes` can validate the proposal programmatically. The create-fallback also surfaces `created_files: [".fallowrc.json"]` so consumers know exactly which paths the apply step would (or did) materialize. Monorepo-subpackage refusals carry `skip_reason: "monorepo_subpackage"` with the resolved `workspace_root`; `--no-create-config` refusals carry `skip_reason: "no_create_config"`. (Refs [#332](https://github.com/fallow-rs/fallow/issues/332).)

- **`fallow list --boundaries --format json` surfaces the user's logical group name for `autoDiscover` zones.** When a config declares `{ name: "features", autoDiscover: ["src/features"] }` (or uses a preset whose zones autoDiscover under the hood, e.g. Bulletproof), expansion previously rewrote `zones[]` into the discovered children (`features/auth`, `features/billing`) and dropped the user-authored `features` parent from the output. Consumers (config UIs, Sankey renderers of `from -> allow` rules, agent-driven config tooling, dashboards) had to reconstruct the grouping from the `features/<child>` naming convention. The output now carries a parallel `logical_groups[]` array with one entry per pre-expansion autoDiscover zone: `name` (the parent the user wrote), `children` (discovered zone names in stable directory-sorted order), `auto_discover` (the user's verbatim path strings, including trailing slash and leading `./`, so config-edit tooling round-trips without rewriting unchanged source), `status` (`ok` / `empty` / `invalid_path`, discriminating "directory empty" from "directory missing or unreadable"), `source_zone_index` (the parent's position in the pre-expansion `zones[]` for byte-accurate config patches), `file_count` (sum across children plus any fallback zone), `merged_from` (parent zone indices when the user declared the same parent name multiple times; surfaces the duplicate in JSON instead of hiding it behind a `tracing::warn!`), `original_zone_root` (echo of the parent's `root` subtree scope so monorepo patchers know whether `root` was authored on the parent or per-child), `child_source_indices` (parallel to `children`, attributing each child to a specific `auto_discover` entry when multiple paths were authored), and optional `authored_rule` (the pre-expansion `{ allow, allowTypeOnly }` keyed on the parent) and `fallback_zone` (cross-reference to the zone twin when the parent had both `patterns` and `autoDiscover`, e.g. the Bulletproof case). `zones[]`, `rules[]`, `zone_count`, `rule_count` are unchanged so existing consumers see zero behavioral delta; the empty-boundaries branch also gains `logical_group_count: 0, logical_groups: []` plus parity `zone_count` / `rule_count` so consumers can read the count fields without first branching on `configured`. The full envelope (`ListBoundariesOutput`, `BoundariesListing`, `BoundariesListZone`, `BoundariesListRule`, `BoundariesListLogicalGroup`, plus the `LogicalGroup`, `LogicalGroupStatus`, `AuthoredRule` building blocks) is now drift-gated in `docs/output-schema.json` via the same Rust-source-of-truth pipeline as the analytical commands (refs [#338](https://github.com/fallow-rs/fallow/issues/338), [#384](https://github.com/fallow-rs/fallow/issues/384)). The human `list --boundaries` output gains a `Logical groups:` section parallel to `Zones:` and `Rules:`, with empty sections suppressed, correct singular/plural counts, errors-first ordering inside the section so misconfigured paths surface at the top, and a `N files (X children + Y fallback)` split when the parent kept its own classifier. (Closes [#373](https://github.com/fallow-rs/fallow/issues/373).)

- **Architecture boundaries: opt-in escape hatch for type-only imports across zones (`allowTypeOnly`).** TypeScript codebases often need type-level contracts between modules that should not have runtime dependencies on each other. Previously the only options were to scatter `// fallow-ignore-next-line` comments on every type-only cross-boundary import (noisy; real violations hide among the legitimate ignores) or duplicate type definitions in a shared layer (loses ownership clarity). A `BoundaryRule` now accepts an optional `allowTypeOnly: ["zoneA", ...]` list mirroring the existing `allow` shape. When `featureB` has `allow: [], allowTypeOnly: ["featureA"]`, type-only imports from `featureB` into `featureA` (`import type {...}`, `import type * as ns`, inline `import { type Foo }` where every named specifier carries the `type` qualifier, AND type-only re-exports `export type { Foo } from "..."`) no longer fire boundary violations. Mixed-specifier imports (`import { type Foo, Bar }`) still fire because the runtime dependency on `Bar` is real. The predicate is `edge.symbols.iter().all(|s| s.is_type_only)`, the same one cycle detection already uses to skip type-only edges from circular-dependency reports. Strictly additive: omitting `allowTypeOnly` preserves pre-feature behavior, and no preset (Layered, Hexagonal, FeatureSliced, Bulletproof) defaults the field on. (Closes [#365](https://github.com/fallow-rs/fallow/issues/365). Thanks [@DrJonki](https://github.com/DrJonki) for the report.)

### Fixed

- **Runtime coverage mapping is tightened along the V8-offset, source-map, and cloud-coverage paths.** Five contributor-PR fixes land together: V8 coverage offsets are now mapped as source positions before the line-and-column lookup so off-by-one ranges no longer misattribute; partial source-map remaps are kept when only some segments resolve cleanly instead of dropping the entire mapping; the owner segment is preserved in source-map project ids so monorepo deployments at `<owner>/<repo>` route to the right Fallow Cloud project instead of being conflated with a same-named repo at a different owner; cloud coverage now requires a line match before attributing a hit (the prior column-only fallback over-credited adjacent statements on a single line); and scoped source-map repo names (`@scope/name`) are accepted by the project-id parser without rejection. (PR [#376](https://github.com/fallow-rs/fallow/pull/376). Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza) for the patch.)

- **Class methods reached via a typed getter chain inside a Playwright fixture teardown are credited.** A Playwright fixture pattern like `await use({ get processEventsService() { return container.resolve(ProcessEventsService); } });` followed by a teardown step that calls `fixtures.processEventsService.clearLast()` previously reported `ProcessEventsService.clearLast` as `unused-class-member` because the typed-getter chain dropped the receiver type before the bound-member-access pass could resolve it. The extract visitor now records typed getter declarations as instance bindings under the same machinery already used for typed fields, so the access chain `<fixtures>.<getter>.<method>` resolves through the existing bound-member pipeline. `CACHE_VERSION` bumped 79 to 80 so the fix takes effect on warm caches. (Closes [#386](https://github.com/fallow-rs/fallow/issues/386). Thanks [@vethman](https://github.com/vethman) for the report.)

- **`docs/output-schema.json` no longer ships an orphan `SuppressAutoFixable` definition.** The committed schema carried a singleton `oneOf: [{type: "string", const: "false"}]` definition named `SuppressAutoFixable` whose prose suggested suppress-action `auto_fixable` should render as a literal `false` rather than a generic boolean. No Rust type ever produced the definition (it survived from a pre-#384 hand-authored draft into the merged regen via `merge_with_committed`), no `$ref` pointed at it, and the wire shape on `SuppressLineAction.auto_fixable` and `SuppressFileAction.auto_fixable` is and remains `{"type": "boolean"}`. Keeping the field as `boolean` is the documented intent on `IssueAction` in `crates/types/src/output.rs:61-64`: a future auto-applier (e.g. an LLM-driven suppression writer) can promote individual variants to `auto_fixable: true` without a schema bump. The definition is removed and the corresponding `HAND_MAINTAINED_ALLOW_LIST` entry in `crates/cli/src/bin/schema_emit.rs` is dropped, so the structural drift gate now rejects re-introduction of the name without a matching Rust source. Strictly an internal schema cleanup: no wire shape change, no codegen-derived TS contract change. (Closes [#402](https://github.com/fallow-rs/fallow/issues/402). Refs [#384](https://github.com/fallow-rs/fallow/issues/384) item 1 follow-up.)

- **`typeof import('./path').X` inside `.d.ts` ambient declarations now traces the target file.** unplugin-auto-import's `auto-imports.d.ts` and unplugin-vue-components' `components.d.ts` embed `typeof import()` references inside `declare global { ... }` and `declare module 'vue' { ... }` bodies; fallow previously walked those bodies but had no extractor for the `TSImportType` node, so the referenced composables and components surfaced as `unused-files` in every Vite + Vue + unplugin-* project. The visitor now records each `TSImportType` as a type-only `ImportInfo` (qualifier root identifier becomes a `Named` import; bare `typeof import('./x')` and `typeof import('./x')['default']` indexed-access patterns produce `SideEffect`), and the graph layer promotes every `.d.ts` / `.d.mts` / `.d.cts` file to an overall reachability root so the new edge actually propagates through BFS. Declaration files were already exempt from `unused-files`; this aligns reachability with that existing "always alive" policy. Side effect: `is_entry_point()` returns `true` for `.d.ts` files, which auto-skips them in `unused-exports` / `unused-types` detection (preventing false positives on module-augmentation interfaces like `GlobalComponents` that TypeScript consumes invisibly via global type resolution). Pass `--include-entry-exports` if you need to re-enable unused-export checking on hand-authored `.d.ts` files. Runtime / test reachability is unchanged: declaration files emit no runtime side effects. `CACHE_VERSION` bumped 81 to 82. (Closes [#396](https://github.com/fallow-rs/fallow/issues/396), [#397](https://github.com/fallow-rs/fallow/issues/397). Thanks [@wouterkroes](https://github.com/wouterkroes) for the reports.)

- **`new URL('./', import.meta.url)` no longer flags `./` as an unresolved import.** The canonical ESM `__dirname` idiom (`fileURLToPath(new URL('./', import.meta.url))`) shipped in nearly every Vite `vite.config.ts` was being treated as a relative module specifier by the `new URL()` worker / asset detector. Directory-only specifiers (`./`, `../`, and any path ending in `/`) are now skipped: they construct a directory URL, not a file URL, and resolve to no module. `./worker.js`, `./assets/foo.svg`, and other file-pointing specifiers are unaffected. (Closes [#399](https://github.com/fallow-rs/fallow/issues/399). Thanks [@wouterkroes](https://github.com/wouterkroes) for the report.)

- **Duplication JSON output now emits `actions: []` on every `CloneGroup` and `CloneFamily`, including the nested `clone_families[].groups[]` items.** The published schema lists `actions` as required on every duplication finding (per the schema-emit augmentation contract introduced in [#338](https://github.com/fallow-rs/fallow/issues/338): the runtime always emits `actions`, so requiring the field on the wire keeps the schema honest). The runtime contract held for top-level `clone_groups[]` and `clone_families[]` but two paths under-emitted: (a) the nested `clone_families[i].groups[j]` items (one `CloneGroup` per inner array element) never received an `actions` field on any code path, and (b) the combined `fallow --format json` builder serialized the duplication sub-block without calling `inject_dupes_actions` at all, so its top-level `dupes.clone_groups[]` and `dupes.clone_families[]` shipped bare too. JSON Schema strict validation of a `fallow dupes --format json` of `vite` reported 234 `required 'actions' is a required property` violations (one per nested group); the combined output reported 608 (top-level families + groups + nested). `inject_dupes_actions` now also walks each family's nested `groups[]` and injects the per-group action set on each inner item, and the combined-mode JSON builder now calls `inject_dupes_actions` on its duplication sub-block. (Closes [#393](https://github.com/fallow-rs/fallow/issues/393).)

- **`docs/output-schema.json` regen no longer silently drops struct fields named after JSON Schema keywords.** `normalize_schema` in `crates/cli/src/bin/schema_emit.rs` strips schemars' integer-width and validation-keyword hints (`format`, `default`, `minimum`, `maximum`, `examples`, `exclusiveMinimum`, `exclusiveMaximum`) so the regenerated document matches the committed canonical form. The recursive walk applied the strip at every map level, including inside `properties` / `definitions` / `$defs` / `patternProperties` maps where the keys are user-facing struct field or type names rather than schema keywords. `ContributorEntry` carries a `pub format: ContributorIdentifierFormat` field (the email-mode discriminator); regeneration silently dropped the property while leaving `"format"` in `required`, which trips ajv's `--strict=true` `strictRequired` check. Both `normalize_schema` and the drift-test mirror `normalize_one` now skip the strip when the key is the parent of a `properties` / `definitions` / `$defs` / `patternProperties` map, so a struct field literally named `format` (or `default` / `minimum` / etc.) survives regen. The committed schema picks up the missing `ContributorEntry.format` property with its `ContributorIdentifierFormat` `$ref`. (Closes [#394](https://github.com/fallow-rs/fallow/issues/394).)

- **`autoDiscover` parent rule now auto-allows discovered child zones, so top-level barrels can re-export feature modules without false-positive cross-zone violations.** When a zone declared both `patterns` and `autoDiscover`, the parent rule's `allow` list was applied verbatim to the parent fallback zone, so a top-level `src/features/index.ts` barrel that re-exported children classified as `features` and surfaced `features → features/<child>` violations. The Bulletproof preset worked around this by leaving `patterns` empty on the `features` zone, which kept top-level files unclassified and unrestricted, including non-barrel files like `src/features/types.ts`. Parent-fallback rule expansion now automatically appends every discovered child to the parent's `allow` while generated child rules keep the original `allow` exactly, preserving sibling-feature isolation. The Bulletproof preset now sets `patterns: ["src/features/**"]` on the `features` zone so top-level feature files classify under the parent zone, barrels can re-export children, and non-barrel top-level files still obey the `features` rule. User configs of the same shape (`{ name: "features", patterns: [...], autoDiscover: [...] }` plus `{ from: "features", allow: [...] }`) inherit the same behavior with no schema change. (Closes [#372](https://github.com/fallow-rs/fallow/issues/372).)

- **PR comment no longer renders empty when the only finding is a project-level dependency / catalog / override issue.** The `--format pr-comment-github` and `--format pr-comment-gitlab` renderers applied a line-based diff filter (`FALLOW_DIFF_FILTER`, default `added`) to every finding, including those anchored on fixed lines inside `package.json` and `pnpm-workspace.yaml`. PRs whose diff did not touch the anchored line, the common shape when a transitive dep drops out of the resolved tree and leaves a stale `pnpm.overrides` entry behind, rendered the comment body as "No GitHub PR/MR findings" while CI still exited non-zero from the same issue surfaced via annotations. Project-level rules (`unused-dependency-override`, `misconfigured-dependency-override`, `unused-catalog-entry`, `empty-catalog-group`, `unresolved-catalog-reference`, `unused-dependency`, `unused-dev-dependency`, `unused-optional-dependency`, `type-only-dependency`, `test-only-dependency`) now bypass the diff filter in the PR-comment path so the body always explains every CI-failure reason. Source-anchored rules continue to honor the filter so comments stay focused on the lines a PR actually changed. The review-envelope path keeps the unconditional filter because inline review comments must anchor on diff lines. (Closes [#381](https://github.com/fallow-rs/fallow/issues/381). Thanks [@cloud-walker](https://github.com/cloud-walker) for the report.)

- **Standalone `export default <template>...</template>` in `.gts` files now extracts imports and the default export.** The v2.75.0 multi-template fix only treated single-byte expression delimiters (`=`, `,`, `(`, `?`, `:`) as expression position, so the canonical template-only-component shape (no `const` wrapper) fell through to blank-out, leaving `export default ;`, a TypeScript syntax error that made oxc bail and drop every import in the file. The stripper now also walks back through identifier bytes and matches against the expression-prefix keyword set `{default, return, throw, yield, await, new}`, with full-identifier comparison so user bindings like `mydefault` or `$return` do not false-positive. `CACHE_VERSION` bumped 78 to 79 so the fix takes effect on warm caches. (Closes [#379](https://github.com/fallow-rs/fallow/issues/379).)

- **Abstract base-class methods called through a generic-constrained `this.<field>` are credited.** A class declared as `BaseService<TClient extends BaseClient>` with `constructor(protected readonly client: TClient)` and a body that calls `this.client.fetchLatest(id)` previously reported `BaseClient.fetchLatest` as an unused class member because the visitor recorded `this.client -> TClient` and `TClient` resolves nowhere. The class visitor now collects `<TypeParam, Constraint>` pairs from `class.type_parameters` on entry and substitutes the constraint when registering `this.<param>` typed bindings, so `this.client.fetchLatest` resolves to `BaseClient.fetchLatest` through the existing bound-member-access pipeline. Unconstrained parameters (`class Container<T>`) drop the binding without effect, matching what fallow already does for any unresolvable type name. Same substitution flows through nested object types and typed getters. (Closes [#388](https://github.com/fallow-rs/fallow/issues/388). Thanks [@vethman](https://github.com/vethman) for the report.)

- **Fluent-builder chains credit intermediate setters, not just the static factory.** `EventBuilder.createWithDefaults().setProcessId("x").setSubject("y").build()` previously flagged every setter and `build()` as unused, because the static-factory machinery (#346) only credited the result of `ID.factory()` when bound to a local variable. The visitor now records each call expression chained off a previous call as a `__fallow_fluent_chain__:` sentinel `MemberAccess` carrying the root `ID`, root method, prior chain prefix, and the method being called now. A new analyze pass walks each chain step against the resolved class export: the root method must be `is_instance_returning_static`, intermediate methods must be `is_self_returning`, and only then is the terminal call credited. Static methods whose declared return type matches the class name are now also treated as factories (covers `static createWithDefaults(): EventBuilder { return chain; }`); instance methods carry a new `is_self_returning` flag set when the declared return type is the class OR when the body's last statement is `return this`. The chain stops at non-self-returning methods like `.build()` returning a different type, so `.build().toString()` does NOT over-credit `EventBuilder.toString`. `CACHE_VERSION` bumped 80 to 81. (Closes [#387](https://github.com/fallow-rs/fallow/issues/387). Thanks [@vethman](https://github.com/vethman) for the report.)

### Documentation

- **TypeScript bare-name backwards-compat aliases ship in the npm-published `fallow/types` contract.** After the #384 / #408 / #409 schema-derive ladder wrapped every bare finding type in a `*Finding` envelope, `json-schema-to-typescript` dropped the orphan bare names because their property set was fully subsumed by the flattening wrapper. The dupes aliases (`CloneGroup`, `CloneFamily`, `AttributedCloneGroup`, `DuplicationReport`) were already re-added at the generator level, but the 17 dead-code bare aliases (`UnusedExport`, `UnusedDependency`, `BoundaryViolation`, etc.) only lived in `editors/vscode/src/types.ts`, which is internal to the VS Code extension and NOT published to npm. External consumers running `import type { UnusedExport } from "fallow/types"` would have failed to type-check after upgrading. `editors/vscode/scripts/codegen-types.mjs` now emits all 17 dead-code bare aliases (plus the 2 union forms `UnusedDependency` and `UnusedMember`) alongside the dupes aliases under a shared `// Backwards-compat aliases` section in the generated `output-contract.d.ts`, so the npm-published and VS Code internal type contracts now carry the same surface. The previously hand-written aliases in `editors/vscode/src/types.ts` are removed; the file re-exports the generated names instead. A `TypeScript bare-name backwards-compat aliases` subsection in `docs/backwards-compatibility.md` documents the policy for public consumers: aliases ship as part of fallow's v2.x stable surface and are scheduled for removal alongside the kind-tagged `FallowOutput` major bump ([#413](https://github.com/fallow-rs/fallow/issues/413)) with a one-minor-cycle deprecation window (`@deprecated` JSDoc + CHANGELOG headline) preceding the removal. No JSON wire shape change. (Closes [#415](https://github.com/fallow-rs/fallow/issues/415).)

- **`fallow explain` for the three complexity rules (`high-cyclomatic-complexity`, `high-cognitive-complexity`, `high-complexity`) now covers synthetic `<template>` and `<component>` Angular findings.** The `RuleDef.full`, `RuleGuide.example`, and `RuleGuide.how_to_fix` strings were function-scoped and read awkwardly for agents acting on Angular findings: a `<template>` row in `findings[]` pointed at prose about "early returns" and "guard clauses", and a `<component>` rollup row pointed at the same. The prose now names both synthetic shapes, lists the template constructs that contribute to cyclomatic (`@if`, `@else if`, `@for`, `@case`, `@defer (when ...)`, legacy `*ngIf`/`*ngFor`, plus ternary and logical operators inside bound attributes and `{{ }}` interpolations) and cognitive (nesting penalties on stacked `@if`/`@for`/`@switch`), explains the rollup math (worst class method's score plus the template's, for each metric), and points at the `component_rollup` payload for per-half breakdown. The same prose also flows through SARIF `fullDescription` (consumed by GitHub Code Scanning, code-quality dashboards) and the MCP `fallow_explain` tool, so AI agents reading either surface get actionable guidance without re-deriving it from JSON. fallow-docs `explanations/health` gains a `Synthetic <template> and <component> findings` subsection under `Complexity metrics` so the docs URLs in `fallow explain` output land on a real anchor. No behavior change. (Closes [#404](https://github.com/fallow-rs/fallow/issues/404).)

- **`auto_fixable` is documented as per-finding, not per action type.** The JSON `actions[]` array has two semantically distinct shapes of `auto_fixable`: per-rule (constant across every finding for an action type) and per-instance (depends on the specific finding's payload). The distinction matters for agents that filter on `auto_fixable: true` to decide what is safe to apply blindly. The enum-level doc on `IssueAction`, the per-field docs on `FixAction.auto_fixable` and `AddToConfigAction.auto_fixable`, and a new `field_definitions` block in the `check` / `health` / `dupes` JSON `_meta` payload now call out per-finding evaluation explicitly and enumerate the four current per-instance flips: `remove-catalog-entry` on `hardcoded_consumers`, the dependency-action `remove-dependency` / `move-dependency` flip on `used_in_workspaces`, `add-to-config` for `ignoreExports` on config-file presence, and `update-catalog-reference` as a forward-compat `false`. The `docs/output-schema.json` descriptions, VS Code + npm typed contracts, fallow-docs `analysis/auto-fix.mdx` + `cli/dead-code.mdx`, and the bundled `fallow-skills` `cli-reference.md` all carry the same wording. No behavior change. (Closes [#361](https://github.com/fallow-rs/fallow/issues/361).)

### Internal

- **CI workflows guard schema drift more aggressively: PR edits to `docs/output-schema.json` now trigger the schema-emit drift gate, and `release.yml` blocks both privileged publish jobs on a fresh `pnpm run check:codegen` pass.** Two follow-ups on the #384 schema-derive ladder. (1) `.github/workflows/ci.yml`'s `rust:` paths-filter (which gates the `check` job that runs `cargo test/clippy --features schema-emit`) now also matches `docs/output-schema.json`, so a PR that touches only the schema document without a matching Rust change still trips the drift gate on PR rather than slipping through to a push-time failure on main. The filter retains the broad `crates/**` scope (instead of a per-crate allow-list) and a comment near the filter enumerates the JsonSchema-deriving subtrees (`crates/types/src/`, `crates/core/src/duplicates/types.rs`, `crates/cli/src/health_types/`, `crates/cli/src/output_envelope.rs`) so future narrowing PRs see the load-bearing scope explicitly. (2) `.github/workflows/release.yml` gains a new `check-codegen` prep-tier job (`contents: read` only, no publish tokens, runs `pnpm run check:codegen` in `editors/vscode/`) that `npm-publish` and `vscode-publish` both depend on. A single `check:codegen` pass validates both committed contracts (`editors/vscode/src/generated/output-contract.d.ts` and `npm/fallow/types/output-contract.d.ts`) because `codegen-types.mjs` writes them in lock-step. The new job sits on the same security boundary as the existing `npm-prep` / `vscode-prep` jobs (install scripts run without tokens, tokens-only jobs never install). (Closes [#416](https://github.com/fallow-rs/fallow/issues/416))

- **MCP params field descriptions are now canonical `///` doc comments only, with a CI drift gate.** Twelve sites in `crates/mcp/src/params.rs` migrate from `#[schemars(description = "...")]` to `///` doc comments so contributors have a single canonical form for field prose. A new test in `crates/mcp/src/server/tests/server_info.rs` (`params_fields_do_not_carry_both_doc_comment_and_schemars_description`) parses the params source and fails the build when a field carries both forms simultaneously: the explicit attribute silently overrides the doc comment in schemars 1, so a later edit to the doc comment would never reach the JSON Schema agents read. The `#[schemars(description = "...")]` escape hatch remains supported when the schema text must intentionally differ from rustdoc, documented in a module-level comment. Wire shape of `tools/list` is unchanged for the affected fields (same names, types, and required flags); description text gained trailing periods and Markdown backticks where prose was reformatted, and a regression test (`converted_field_descriptions_render_in_schema`) pins that descriptions continue to render non-empty. (Closes [#421](https://github.com/fallow-rs/fallow/issues/421))

- **Inner complexity-finding struct renamed from `HealthFinding` to `ComplexityViolation` at the Rust level only; the public schema and TS surface stay as `HealthFinding`.** Prerequisite for the upcoming #384 B2 wrapper migration, which introduces a new `HealthFinding` wrapper that flattens the inner struct via `#[serde(flatten)]` and adds typed `actions` + `introduced` natively, retiring `inject_health_actions` from the JSON post-pass. Renaming the inner up front avoids landing the wrapper under a `HealthFindingFinding` collision. The struct and every Rust consumer in `crates/cli` and `crates/types` flip to `ComplexityViolation`, but `schemars(rename = "HealthFinding")` pins the public schema definition name and the codegen-derived TS contracts (`docs/output-schema.json`'s `$ref` graph, `editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts` published as `fallow/types`) all continue to expose the type as `HealthFinding`. At B2 the `schemars(rename)` attribute drops off and the public name migrates from this inner type to the wrapper, keeping the schema definition name stable across both PRs. JSON wire-level field name on `HealthReport.findings[]` (and its grouped / audit / combined echoes) is controlled by serde, so wire shape stays byte-identical; verified against the `vue-core` benchmark fixture (`total_issues`, `findings[]` ordering, and per-finding `actions[]` discriminants identical, only `elapsed_ms` differs). `HealthFindingAction` and `HealthFindingActionType` keep their names natively; they describe actions attached to the finding and naturally fit the future wrapper. (Refs [#384](https://github.com/fallow-rs/fallow/issues/384))

- **Catalog and dependency-override findings now flow through typed `*Finding` envelope wrappers, retiring the legacy `inject_actions` JSON post-pass.** Continues the schema-derive ladder for item 1 of #384: six wrappers in `crates/types/src/output_dead_code.rs` (`DuplicateExportFinding`, `UnusedCatalogEntryFinding`, `EmptyCatalogGroupFinding`, `UnresolvedCatalogReferenceFinding`, `UnusedDependencyOverrideFinding`, `MisconfiguredDependencyOverrideFinding`) flatten the bare findings via `#[serde(flatten)]` and populate a typed `actions: Vec<IssueAction>` array at construction time. Data-dependent semantics that previously lived in the post-pass move into each wrapper's `with_actions` constructor: `UnusedCatalogEntryFinding` flips per-instance `auto_fixable` based on `hardcoded_consumers`, `UnresolvedCatalogReferenceFinding` discriminates between `add-catalog-entry` and `update-catalog-reference` at position 0 with optional `suggested_target` (a new field on `FixAction` for the unambiguous-single-alternative case), `DuplicateExportFinding` derives the position-0 `add-to-config` ignoreExports value from the finding's `locations[]` paths and exposes a `set_config_fixable` mutator the CLI's JSON serializer calls with the fix-applier's readiness signal, and both override wrappers omit the `add-to-config` suppress entirely when neither `target_package` nor `raw_key` yields a usable package name. `AnalysisResults`'s six matching fields flip from bare `Vec<Inner>` to `Vec<*Finding>`; construction wraps at `find_dead_code_full` in `crates/core/src/analyze/mod.rs`. The full ~400-line `inject_actions` scaffolding in `crates/cli/src/report/json.rs` (`actions_for_issue_type`, `build_actions`, `build_unresolved_catalog_reference_primary_action`, `build_duplicate_exports_config_value`, `is_dependency_issue`, the `SuppressKind` and `ActionSpec` types, the three `IGNORE_*_VALUE_SCHEMA` URL constants, and the `inject_actions` walker itself) is deleted; the grouped-JSON and single-JSON call sites no longer post-process the serialized value. Five named position-0 invariant tests in `crates/types/src/output_dead_code.rs` document the load-bearing `actions[0].type` semantics that downstream consumers (GitHub Action / GitLab CI jq scripts, MCP `actions[0].type` dispatch, VS Code LSP code-action rendering) depend on, separately from the omnibus snapshot tests. `schema_emit.rs` drops the six bare names from `finding_definition_names()` so the bare types lose the `actions` + `introduced` augmentation, and the wrappers carry both fields natively via schemars; `docs/output-schema.json` gains six `*Finding` definitions, and the bare `DuplicateExport` / `UnusedCatalogEntry` / `EmptyCatalogGroup` / `UnresolvedCatalogReference` / `UnusedDependencyOverride` / `MisconfiguredDependencyOverride` types stay in the registry without the wrapper fields so external consumers can still read the unwrapped shape. Generated TS contracts (`editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts`) regenerated. Wire shape stays byte-identical (verified against the `vue-core` benchmark fixture). With this batch the legacy `inject_actions` post-pass is fully retired; `inject_health_actions` and `inject_dupes_actions` for the health and duplication families remain on the post-pass path until their own typed-wrapper migrations land. (Refs [#384](https://github.com/fallow-rs/fallow/issues/384))

- **Document-root `oneOf` in `docs/output-schema.json` is now derived from a typed `FallowOutput` enum.** Every object-shaped `--format json` envelope (`AuditOutput`, `CheckOutput`, `CheckGroupedOutput`, `CombinedOutput`, `DupesOutput`, `HealthOutput`, `ExplainOutput`, `CoverageSetupOutput`, `ReviewEnvelopeOutput`, `ReviewReconcileOutput`, `ListBoundariesOutput`) is a variant of a typed `FallowOutput` enum in `crates/cli/src/output_envelope.rs`, derived via schemars; the schema-emit binary's `rewrite_document_root_one_of` step replaces the previously hand-maintained 12-entry root block. The regenerated TypeScript contracts (`editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts`) now expose a `FallowOutput` discriminated union for downstream codegen consumers. Wire compatibility: the top-level shape of every envelope is unchanged byte-for-byte; consumers continue to narrow by unique field presence (`summary.total_issues` for check, `health_score` for health, `clone_groups` for dupes, `check`+`dupes`+`health` keys together for the bare combined invocation, `boundaries` for list --boundaries, `command: "audit"` for audit). `HealthOutput` and `DupesOutput` flatten their body (`HealthReport`/`DuplicationReport`) into top-level fields, so the discriminator is a body field, not a wrapper key. `CodeClimateOutput` stays as a sibling root branch because the Code Climate / GitLab Code Quality spec requires a bare JSON array at the root, which cannot be a variant of `FallowOutput` once the planned major-bump migration adds a top-level `kind` discriminator field that requires every variant to serialize as an object. `CoverageAnalyzeOutput` remains hand-maintained pending #384 item 3c; a new `HAND_MAINTAINED_ROOT_ENVELOPES` constant plus `hand_maintained_root_envelopes_appear_in_root_one_of` drift test ensure it stays reachable from the documented union until the typed migration lands. A future major release plans to add a top-level `kind` discriminator field for true O(1) narrowing on AI / agent consumers, paired with a one-cycle `--legacy-envelope` opt-out flag so existing consumers can migrate. (Closes [#384](https://github.com/fallow-rs/fallow/issues/384).)

- **Dependency-family dead-code findings now flow through typed `*Finding` envelope wrappers.** Continues the schema-derive ladder for item 1 of #384: six wrappers in `crates/types/src/output_dead_code.rs` cover the dependency family (`UnusedDependencyFinding`, `UnusedDevDependencyFinding`, `UnusedOptionalDependencyFinding` over the shared `UnusedDependency` struct; `UnlistedDependencyFinding` over `UnlistedDependency`; `TypeOnlyDependencyFinding` and `TestOnlyDependencyFinding` over their respective bare structs). The three `UnusedDependency` views share `build_unused_dependency_actions` which swaps the primary fix from `remove-dependency` to `move-dependency` when `dep.used_in_workspaces` is non-empty (the cross-workspace discriminant). The four `Add to ignoreDependencies` suppress actions share `build_ignore_dependencies_suppress_action`. `AnalysisResults.unused_dependencies` / `unused_dev_dependencies` / `unused_optional_dependencies` / `unlisted_dependencies` / `type_only_dependencies` / `test_only_dependencies` flip from bare `Vec<T>` to `Vec<*Finding>`; construction wraps at `find_dead_code_full` in `crates/core/src/analyze/mod.rs`. The six entries retire from `actions_for_issue_type` / `inject_actions` in `crates/cli/src/report/json.rs`, and the now-unused `SuppressKind::ConfigIgnoreDep` variant plus its match arm and the `IGNORE_DEPENDENCIES_VALUE_SCHEMA` constant are deleted as dead code. `schema_emit.rs` drops the six bare names from `finding_definition_names()` and registers the six wrappers in `derived_definitions()`. Consumers across audit, baseline, regression, codeclimate / sarif / compact / markdown / human-output / json formatters, LSP diagnostics (with `Box<dyn Iterator>` unification for the three heterogeneous dependency view types), fix orchestrator, and the snapshot test suite now access fields via `entry.dep.*` (or pass `&entry.dep` to bare-typed helpers). Wire shape stays byte-identical (verified on vite / preact / zod fixtures: 2.9 MB, 1.5 MB, 2.2 MB JSON outputs respectively, all identical after stripping `elapsed_ms` / `generated_at`); `docs/output-schema.json` gains six `*Finding` definitions, the bare `UnusedDependency` / `UnlistedDependency` / `TypeOnlyDependency` / `TestOnlyDependency` types lose their `actions` + `introduced` augmentation, and the `AnalysisResults` field `items` refs flip to the wrappers. Generated TS contracts (`editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts`) regenerated. (Refs [#384](https://github.com/fallow-rs/fallow/issues/384))

- **CodeClimate, review-envelope, and coverage-setup wire emit now flows through typed envelope structs.** `crates/cli/src/report/codeclimate.rs` builds `Vec<CodeClimateIssue>` instead of `Vec<serde_json::Value>` (the `cc_issue` helper returns `CodeClimateIssue`, `severity_to_codeclimate` returns `CodeClimateSeverity`, all per-rule push helpers and the three `build_*_codeclimate` aggregators thread the typed vec through; a new `issues_to_value` helper centralises the `serde_json::to_value` conversion at every `print_*` / audit / combined / PR-comment / review-envelope boundary). `crates/cli/src/report/ci/review.rs::render_review_envelope` returns `ReviewEnvelopeOutput` and `render_comment` returns `ReviewComment::GitHub` / `ReviewComment::GitLab` instead of `serde_json::json!` literals. `crates/cli/src/coverage/mod.rs::build_setup_envelope` constructs `CoverageSetupOutput` end-to-end (with `FrameworkKind` / `PackageManager` / runtime-target enum conversions) so `build_setup_json` is now a thin `serde_json::to_value` wrapper. The file-wide `#![allow(dead_code)]` on `crates/cli/src/output_envelope.rs` is gone; per-struct `#[allow(dead_code)]` annotations scope the suppression to the remaining schema-source-of-truth-only envelopes (`AuditOutput`, `CodeClimateOutput` wrapper, `ListBoundariesOutput` family) until follow-up migrations land. Two unused string-returning helpers (`severity::codeclimate_severity`, `severity::github_check_conclusion`) were removed; the typed enums replace their callsites. Wire output is byte-identical (verified against `fallow --format codeclimate` on the `preact` benchmark and `fallow coverage setup --json` on a Next.js fixture). (Refs [#384](https://github.com/fallow-rs/fallow/issues/384) items 3a / 3b / 3c.)

- **1:N dead-code findings (UnusedExport/UnusedType, UnusedEnumMember/UnusedClassMember) now flow through typed `*Finding` envelope wrappers.** Continues the schema-derive ladder for item 1 of #384: two Rust structs (`UnusedExport`, `UnusedMember`) each back two wrappers, one per issue-key view (`unused_exports` vs `unused_types`, `unused_enum_members` vs `unused_class_members`). The wrappers diverge in their fix actions: `UnusedExportFinding` emits `remove-export` with the public-API description, `UnusedTypeFinding` emits `remove-export` with the type-declaration description; both pick up the `is_re_export`-aware public-API note swap inside `with_actions` (replacing the post-pass `note` rewrite in `report/json.rs`). `UnusedEnumMemberFinding` emits `remove-enum-member` auto-fixable, `UnusedClassMemberFinding` emits `remove-class-member` with the dependency-injection caveat note and `auto_fixable: false`. `AnalysisResults.unused_exports` / `unused_types` / `unused_enum_members` / `unused_class_members` flip from bare `Vec<T>` to `Vec<*Finding>`; construction wraps at `find_dead_code_full` in `crates/core/src/analyze/mod.rs` so the detectors keep returning bare Vecs and the orchestrator maps through each wrapper's `with_actions` before assembly. `suppress_signature_backing_types` runs on the bare Vec before wrapping (function signature unchanged). The four entries retire from `actions_for_issue_type` / `inject_actions` in `crates/cli/src/report/json.rs`, including the `is_re_export` note-swap branch. `schema_emit.rs` drops `UnusedExport` and `UnusedMember` from the augmentation table and registers the four wrappers in `derived_definitions()`. Consumers across audit, baseline, regression, codeclimate / sarif / compact / markdown / human-output / json formatters, LSP diagnostics / quick-fix / hover, and the snapshot test suite now access fields via `entry.export.*` / `entry.member.*` (or pass `&entry.export` / `&entry.member` to bare-typed helpers). Wire shape stays byte-identical (8834 tests pass); `docs/output-schema.json` gains four `*Finding` definitions, the bare `UnusedExport` and `UnusedMember` types lose their `actions` + `introduced` augmentation, and the `AnalysisResults` field `items` refs flip to the wrappers. Generated TS contracts (`editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts`) regenerated. (Refs [#384](https://github.com/fallow-rs/fallow/issues/384))

- **Simple 1:1 dead-code findings now flow through typed `*Finding` envelope wrappers.** Continues the schema-derive ladder for item 1 of #384: `UnusedFile`, `PrivateTypeLeak`, `UnresolvedImport`, `CircularDependency`, and `BoundaryViolation` each gain a typed wrapper in `crates/types/src/output_dead_code.rs` that flattens the bare finding via `#[serde(flatten)]` plus a typed `actions: Vec<IssueAction>` populated at construction time, with `introduced: Option<AuditIntroduced>` carrying the optional audit breadcrumb so the schema renders the field via `$ref: #/definitions/AuditIntroduced`. `AnalysisResults` flips the five matching `Vec<Bare>` fields to `Vec<*Finding>`; construction wraps at `find_dead_code_full` in `crates/core/src/analyze/mod.rs`. The `actions_for_issue_type` / `inject_actions` post-pass in `crates/cli/src/report/json.rs` retires the five entries, and `crates/cli/src/bin/schema_emit.rs` drops the same five bare names from `finding_definition_names()` / `finding_augmentation()` and registers the new wrappers in `derived_definitions()`. `crates/types/src/output` is no longer feature-gated so `IssueAction` is available at runtime; `serde_json` becomes a hard dep for the types crate. Wire shape is byte-identical (8834 tests still pass); `docs/output-schema.json` gains five `*Finding` definitions with native `actions` + `introduced` properties and drops the augmentation on the bare types. Generated TS contracts (`editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts`) regenerated. The `SuppressKind::FileComment` branch and the now-empty `actions_for_issue_type` arms for the five retired keys are deleted as dead code. (Refs [#384](https://github.com/fallow-rs/fallow/issues/384))

- **`UntestedFile` and `UntestedExport` finding actions now flow through typed `UntestedFileFinding` / `UntestedExportFinding` envelope wrappers.** First stage of item 1 in the schema-derive ladder: source structs stay bare; new envelope wrappers in `crates/cli/src/health_types/coverage.rs` carry the inner finding via `#[serde(flatten)]` plus a typed `actions: Vec<UntestedXAction>` populated at construction time. `compute_coverage_gaps` in `crates/cli/src/health/scoring.rs` now takes `root: &Path` so action descriptions embed the project-root-relative path inline (matching the `strip_root_prefix` post-pass output on the inner `path` field). The Value-walking `build_untested_file_actions` / `build_untested_export_actions` post-pass is retired; `UntestedFile` and `UntestedExport` are removed from `finding_definition_names()` / `finding_augmentation()` in `schema_emit.rs`. `crates/types/src/output_health` is no longer feature-gated so the typed action structs are available at runtime. Wire shape stays byte-identical; `docs/output-schema.json` gains the `UntestedFileFinding` / `UntestedExportFinding` definitions, drops the `actions` augmentation on `UntestedFile` / `UntestedExport`, and flips `CoverageGaps.files.items` / `exports.items` `$ref` to the wrappers. (Refs [#384](https://github.com/fallow-rs/fallow/issues/384))

- **`RuntimeCoverageReport.schema_version` is now derived from a typed `RuntimeCoverageSchemaVersion` enum.** The post-derivation `augment_runtime_coverage_report` graft in `crates/cli/src/bin/schema_emit.rs` and the matching `inject_runtime_coverage_report_schema_version` / `inject_runtime_schema` post-pass in `crates/cli/src/report/json.rs` and `crates/cli/src/coverage/analyze.rs` are retired; the field flows through schemars natively and serde keeps it first on the wire. `docs/output-schema.json` gains a `RuntimeCoverageSchemaVersion` definition that mirrors the `CoverageSetupSchemaVersion` / `ReviewEnvelopeSchema` precedent, and `RuntimeCoverageReport.properties.schema_version` becomes a `$ref` instead of an inline `enum: ["1"]` graft. Generated TS contracts (`editors/vscode/src/generated/output-contract.d.ts`, `npm/fallow/types/output-contract.d.ts`) regenerated. Closes the `MAINTENANCE:` coupling that required hand-editing the schema graft whenever the runtime coverage contract version bumped. (Refs [#384](https://github.com/fallow-rs/fallow/issues/384))

- **Schema drift gate now covers every committed definition, not just the 105-entry explicit allow-list.** The strict structural gate (`drift_tests::committed_definitions_match_derived_structurally`) walks every key emitted by `derived_definitions()` and compares it against the committed entry, with a separate orphan check that fails when `docs/output-schema.json` lists a definition schemars no longer produces. The expanded coverage caught five hand-maintained orphans that the loose gate had silently preserved across regens: `AttributedCloneGroup`, `AttributedInstance`, and `DuplicationGroup` now derive from their Rust source (`crates/cli/src/report/dupes_grouping.rs`) with matching `JsonSchema` derives and `derived_definition_names()` registration; the stale `SuppressAutoFixable` singleton (defined but never `$ref`'d) is removed; and the three legitimately hand-maintained refs (`CloneFamilyAction`, `CloneGroupAction`, `CoverageAnalyzeOutput`) are now declared via a typed `HAND_MAINTAINED_ALLOW_LIST` with each entry linked to the meta-issue ladder rung that retires it (typed action wrappers, typed envelope builders). `AttributedCloneGroup` joins the augmentation map so its `actions` array references `CloneGroupAction` instead of being an untyped breadcrumb. (Refs [#384](https://github.com/fallow-rs/fallow/issues/384))

- **`docs/output-schema.json` now carries a stable `$id` so consumers can SHA-pin a specific schema revision.** The top-level identifier is `https://raw.githubusercontent.com/fallow-rs/fallow/main/docs/output-schema.json`. Replace `main` with a release tag (for example `v2.75.0`) or commit SHA to pin. ajv and other validators do not fetch `$id` over the network by default; the URL functions as a deduplication key for `addSchema` and as a base URI for `$ref` resolution. The pinning contract, a minimal ajv strict setup snippet, and the relationship to the generated TS types (`npm/fallow/types/output-contract.d.ts`) are documented in `docs/backwards-compatibility.md`. (Refs [#384](https://github.com/fallow-rs/fallow/issues/384))

- **`docs/output-schema.json` is now derived from Rust types as the source of truth.** Every definition registered in `derived_definition_names()` (`crates/cli/src/bin/schema_emit.rs`) is generated by schemars off `#[derive(JsonSchema)]` on the matching Rust struct; the `fallow-schema-emit` binary (gated on the `schema-emit` cargo feature) regenerates the committed file and a CI drift gate (`cargo test -p fallow-cli --features schema-emit --bin fallow-schema-emit`) fails on any structural divergence. The migration covers:
  - **Health subtree**: `HealthFinding`, `HealthSummary`, `HealthScore` + `HealthScorePenalties`, `VitalSigns` + `VitalSignsCounts` + `RiskProfile`, `HotspotEntry` + `HotspotSummary` + `OwnershipMetrics` + `ContributorEntry`, `RefactoringTarget` + `TargetThresholds`, `HealthTrend` + `TrendCount`, `FileHealthScore`, `LargeFunctionEntry`, `CoverageGaps` + `CoverageGapSummary` + `UntestedFile` + `UntestedExport`, and the full `RuntimeCoverageReport` subtree (`RuntimeCoverageSummary`, `RuntimeCoverageCaptureQuality`, `RuntimeCoverageEvidence`, `RuntimeCoverageAction`, `RuntimeCoverageMessage`, `RuntimeCoverageFinding`, `RuntimeCoverageHotPath`, `RuntimeCoverageBlastRadiusEntry`, `RuntimeCoverageImportanceEntry`, plus the protocol-derived signal / verdict / watermark / confidence / risk-band enums).
  - **Per-finding action wrappers** typed in `fallow_types::output_health` (`HealthFindingAction`, `HotspotAction`, `RefactoringTargetAction`, `UntestedFileAction`, `UntestedExportAction`), so the action arrays attached to health findings, hotspots, refactoring targets, and coverage-gap items are now drift-checked.
  - **Envelope utility shapes** in `fallow_types::envelope` (`SchemaVersion`, `ToolVersion`, `ElapsedMs`, `AuditIntroduced`, `EntryPoints`, `CheckSummary`, `BaselineDeltas` + `BaselineCategoryDelta` + `BaselineMatch`, `RegressionResult` + `RegressionStatus` + `RegressionToleranceKind`, `Meta` + `MetaMetric` + `MetaRule`).
  - **Per-command envelopes** in `crates/cli/src/output_envelope.rs` (`AuditOutput`, `CheckOutput`, `CheckGroupedOutput` + `CheckGroupedEntry`, `CombinedOutput`, `DupesOutput`, `HealthOutput` + `HealthGroup`, `ExplainOutput`, `CodeClimateOutput` + `CodeClimateIssue` + `CodeClimateLocation` + `CodeClimateLines`, `ReviewEnvelopeOutput` + `GitHubReviewComment` + `GitLabReviewComment` + `GitLabReviewPosition` + `ReviewEnvelopeMeta`, `ReviewReconcileOutput`, `CoverageSetupOutput` + `CoverageSetupMember` + `CoverageSetupFileToEdit` + `CoverageSetupSnippet`) along with the enum discriminators (`AuditVerdict`, `AuditCommand`, `CoverageSetupFramework`, `CoverageSetupPackageManager`, `CoverageSetupRuntimeTarget`, `GroupByMode`).
  - **Body shapes**: `DuplicationReport` (`crates/core/src/duplicates/types.rs`) and `HealthReport` (`crates/cli/src/health_types/`) are registered so the `$ref` targets on `CombinedOutput.dupes` / `health` and `AuditOutput.duplication` / `complexity` resolve against the bare body the wire actually emits.
  - **Fields previously emitted but missing from the schema** are now documented: `HotspotEntry.is_test_path`, `OwnershipMetrics.suggested_reviewers`, the four `VitalSignsCounts` fields (`duplicated_lines` / `total_lines` / `files_scored` / `total_deps`), the `actions` arrays on `UntestedFile` / `UntestedExport`. `OwnershipMetrics.unowned` and `RuntimeCoverageSummary.last_received_at` drop from `required` to match their `Option<T>` sources.
  - The `crates/cli/src/report/json.rs` emission path is unchanged; the new types are schema-only for this PR. (Refs [#338](https://github.com/fallow-rs/fallow/issues/338))

- **The strict structural drift gate is now active and `docs/output-schema.json` is fully regenerated from Rust.** `drift_tests::committed_definitions_match_derived_structurally` runs on every `cargo test` (no `#[ignore]`), so any hand edit to the in-scope definitions in `docs/output-schema.json` fails CI. Closes the prose-and-shape escape hatch the prior schema-derive bullet opened. Highlights:
  - **Augmentation correctness**: `augment_finding_definition` now pushes `"actions"` into every finding type's `required` array (the wire always emits the array, possibly empty); the `actions_meta` breadcrumb on `HealthReport` is modeled as a typed `Option<HealthActionsMeta>` instead of the post-pass JSON-value injection key.
  - **Schemars cosmetic strip on the production path**: `normalize_schema` drops `default` / `examples` / `format` / `minimum` / `maximum` / `exclusiveMinimum` / `exclusiveMaximum` and collapses single-arm `allOf:[{$ref}]` wrappers so the regenerated document stays clean. Mirrors what the test-side normalizer already strips so the gate is symmetric.
  - **Schemars title preservation on envelopes**: each top-level envelope (`AuditOutput`, `CheckOutput`, `CombinedOutput`, etc.) carries `#[schemars(title = "fallow <command> --format json")]` so the Mintlify schema renderer keeps its section headings.
  - **Helper-definition merge correctness**: `merge_with_committed`'s helper loop now always overwrites transitive helpers from the derived schema rather than silently preserving the prior committed entries; this fixed a `nest_js` / `svelte_kit` -> `nestjs` / `sveltekit` rename mismatch on `CoverageSetupFramework` that ajv strict would have rejected on every real NestJS / SvelteKit `coverage setup --json` output.
  - **Latent schema bug**: the committed schema's `CoverageSetupOutput.package_manager` previously encoded `{ "type": "string", "enum": [null, ...] }` which is invalid against draft-07 strict. The regen replaces it with the proper `anyOf: [{$ref}, {type: "null"}]` form.
  - **Prose preserved**: every previously-published per-field description is restored by enriching the `///` doc comments on the matching Rust source struct (141 field descriptions across 42 definitions).
  - VS Code + npm typed contracts (`output-contract.d.ts`) regenerated via `pnpm run codegen:types`. `CONTRIBUTING.md` updated to reflect the closed escape hatch. (Refs [#338](https://github.com/fallow-rs/fallow/issues/338))

## [2.75.0] - 2026-05-16

### Added

- **AdonisJS plugin (v5/v6/v7) added under the framework category.** Fallow now understands the conventional AdonisJS folder layout (controllers, models, middleware, validators, services, providers, preloads, commands, configs, contracts, database migrations / seeders / factories), bootstrap files (`server.{ts,js}`, `ace`, `bin/**/*.{ts,js}`), the `@ioc:` virtual import prefix in v5, and the framework-managed peer packages that ship via the runtime container. `resolve_config` understands both rc shapes: `.adonisrc.json` (v5, parsing `preloads`, `providers`, `commands`, `aceProviders` in both string and `{ file, environment }` forms, `aliases`, `metaFiles[].pattern`, and `types[]`) and `adonisrc.ts` (v6/v7, walking `defineConfig({...})` for thunk-wrapped lazy imports plus the assembler hook arrays under `hooks.*`, applying `directories.*` overrides as entry patterns, and feeding the project's `package.json#imports` into the resolver's path-alias table). Built-in plugin count goes from 94 to 95. As part of this, `fallow-extract` exposes three shared dynamic-import peel helpers (`extract_import_expression`, `extract_import_from_return_body`, `extract_import_from_callable`) so the v6/v7 rc parser and existing `try_extract_arrow_wrapped_import` / `try_extract_property_callback_import` visitor sites share one canonical peel path. Real-project validation (release build, `total_issues / unused_files`): AdonisJS 6 graphql playground 68/42 to 17/3, openapi playground 55/34 to 14/1, web-starter-kit 32/19 to 11/2, api-starter-kit 27/15 to 9/0, inertia-starter-kit 32/18 to 10/0, slim-starter-kit 22/13 to 9/0. Zero regression on zod, preact, vite, next.js. (PR [#364](https://github.com/fallow-rs/fallow/pull/364). Thanks [@AlphaLawless](https://github.com/AlphaLawless) for the patch.)

### Fixed

- **`.gts` files with multiple `<template>` blocks now extract imports and exports correctly.** Previously the `<template>` stripper blanked every block to spaces, which is fine for class-body templates (an empty class body is valid) but produces `const x = ;` for module-level template expressions, a TypeScript syntax error that caused oxc to bail and drop every import in the file. Real Ember/Glimmer route templates that combine an inline `TemplateOnlyComponent` with a class-body template were silently parsing as zero-edge files, and every component they referenced was reported as `unused-file`. The stripper now detects expression position (previous non-whitespace byte is `=`, `,`, `(`, `?`, or `:`) and replaces the block with a byte-length-preserving parenthesized template literal so the surrounding statement stays syntactically valid. Class-body templates retain the existing blank-out behavior. Marking is plumbed through the extract cache (`CACHE_VERSION` 77 to 78) so the fix takes effect on warm caches. (Closes [#375](https://github.com/fallow-rs/fallow/issues/375). Thanks [@ShockwaVee](https://github.com/ShockwaVee) for the report.)

- **`vi.mock()` without a factory no longer surfaces a phantom `unresolved-import` pointing at a `__mocks__/` path the user never wrote.** Fallow synthesises a `<dir>/__mocks__/<file>` dynamic import next to every `vi.mock('./foo')` call so vitest's optional manual-mock convention credits the sibling when it does exist. When the sibling did NOT exist on disk, the synthesised path flowed through to `unresolved-imports` as if the user had typed it, producing findings like `@/utils/__mocks__/exportElementAsPng` for projects that rely on vitest's in-memory auto-mocking. The synthesised entry is now marked speculative and the resolver drops it silently when the target can't be found; the credit path is unchanged (a `__mocks__/<file>` that DOES exist on disk is still credited as referenced). Marking is plumbed through the extract cache (`CACHE_VERSION` 76 to 77) so the change takes effect even on warm caches. (Closes [#377](https://github.com/fallow-rs/fallow/issues/377) and [#378](https://github.com/fallow-rs/fallow/issues/378). Thanks [@cloud-walker](https://github.com/cloud-walker) for the report.)

- **Istanbul `--coverage` now matches functions written as multiline typed async arrows.** Istanbul producers (Jest, nyc, c8, babel-plugin-istanbul) are inconsistent about `FnEntry.line`: some emit the declaration line, others the body start. Fallow extracts function positions at the declaration, so multiline TS signatures like `export const elementsFrom = async (...): Promise<T> => {...}` lost their coverage match whenever the producer pointed `line` at the body. The loader now indexes both the producer's effective line and the declaration start as aliases so lookups by declaration position resolve through the exact and name-fuzzy paths, and the anonymous-by-position fallback is guarded by a column-distance cap so the new aliases never credit unrelated functions sitting above a multiline arrow. (Closes [#370](https://github.com/fallow-rs/fallow/issues/370). Thanks [@Guria](https://github.com/Guria) for the report.)

## [2.74.0] - 2026-05-14

### Added

- **Architecture boundary auto-discovery (`autoDiscover` on `BoundaryZone`).** A zone declaring `autoDiscover: ["src/features"]` expands at config-resolve time into one child zone per immediate child directory, named `<parent>/<child>` (e.g. `features/auth`, `features/billing`). Rules that reference the logical parent (`from: "features"`) expand to every discovered child, so a single rule declaration polices every feature; explicit child rules (`from: "features/auth"`) override the generated parent rule regardless of declaration order. The Bulletproof preset now uses `autoDiscover` for the `features` zone by default, so a fresh `preset: "bulletproof"` config gives sibling-feature isolation with zero per-feature boilerplate. A `src/features/index.ts` barrel that re-exports its children stays unclassified (and unrestricted) by design so the barrel does not produce false-positive `features -> features/<child>` violations; the trade-off is that other top-level files in `src/features/` are also unrestricted. A user-defined zone that sets BOTH `patterns` and `autoDiscover` triggers a once-per-process `tracing::warn!` because the parent fallback typically produces false positives on barrels. `BoundaryZone.patterns` is now optional in `.fallowrc.json` (a zone may define `patterns`, `autoDiscover`, or both). Follow-ups [#372](https://github.com/fallow-rs/fallow/issues/372) tracks differentiated parent-vs-child rule expansion for strict mode on non-barrel top-level files, and [#373](https://github.com/fallow-rs/fallow/issues/373) tracks surfacing the logical-group name in `list --boundaries` JSON. (Closes [#368](https://github.com/fallow-rs/fallow/issues/368). Thanks [@DrJonki](https://github.com/DrJonki) for the report.)

- **`fallow fix` now auto-applies `ignoreExports` rules to the active fallow config.** Previously the only way to silence an unused-export finding was a per-file `// fallow-ignore-next-line` comment or hand-editing `ignoreExports`. The fixer now writes the suppression directly into the active config (`.fallowrc.json` / `.fallowrc.jsonc` / `fallow.toml` / `.fallow.toml`) via the same format-preserving writer that backs every `add-to-config` action: round-trips JSONC comments and CRLF line endings, preserves symlinked configs, follows `--config <path>` overrides, and deduplicates absolute-path entries against existing relative ones. The CRLF-doubling bug surfaced by the panel review during implementation is locked down by a regression test; the same writer is now the canonical path for every `add-to-config` action across rule families. (Closes [#366](https://github.com/fallow-rs/fallow/issues/366).)

- **Detects empty named pnpm catalog groups in `pnpm-workspace.yaml` and auto-fixes them via `fallow fix` or an LSP quick-fix.** When `fallow fix` removes the last entry from a named catalog group, the v2.73.0 fixer rewrites the header to `catalogs.<name>: {}` so pnpm stays happy, but the named-catalog placeholder lingers in the file as lint-rot in long-lived monorepos. The new `empty-catalog-group` rule (default `warn`) flags every named group under `catalogs:` that has no package entries, with severity `warn` because removing them is a hygiene cleanup rather than a correctness fix. The top-level `catalog:` map is intentionally never flagged because users keep it as a stable hook. `fallow fix` deletes only the named `catalogs.<name>:` header line and leaves comments plus the parent `catalogs:` block intact; catalog-entry leading-comment deletion is controlled separately by `fix.catalog.deletePrecedingComments`. Multi-document YAML files (`---` separators) are rejected with a skip record; the post-edit content is reparsed via `serde_yaml_ng` before persisting. Findings render in all six report formats (human, JSON, SARIF `fallow/empty-catalog-group`, compact, markdown, CodeClimate), the LSP emits a matching `WARNING` diagnostic, and a `Remove empty catalog group \`<name>\`` quick-fix code action is offered for each finding with anchored key-prefix matching so sibling headers with shared prefixes (`react17` vs `react18`) cannot be deleted by mistake. The MCP `analyze` tool accepts `issue_types: ["empty-catalog-groups"]`, the GitHub Action and GitLab CI jq scripts surface the new field in summary tables and emit `::warning` annotations, and `fallow explain empty-catalog-group` opens with the rationale and a fixed example. Suppression is via YAML comment (`# fallow-ignore-next-line empty-catalog-group`) or rule severity (`empty-catalog-groups: "off"`). New `IssueKind::EmptyCatalogGroup` (discriminant 25); new `EmptyCatalogGroup` struct on `AnalysisResults`. (Closes [#359](https://github.com/fallow-rs/fallow/issues/359))

### Fixed

- **`unused-dependency-overrides` no longer false-positives on transitive-only overrides resolved through `pnpm-lock.yaml`.** Overrides that pin a CVE-fix version of a purely transitive dependency (no direct dep in any workspace `package.json`) were flagged as unused even though pnpm resolved them at install time. The detector now consults `pnpm-lock.yaml` to confirm whether the lockfile resolved a version that satisfies the override; an override is reported as unused only when neither a workspace `package.json` declares the target nor the lockfile resolves it. Parent-chain shapes (`react>react-dom: ^17`) inherit the same lockfile awareness. (Closes [#371](https://github.com/fallow-rs/fallow/issues/371). Thanks [@cloud-walker](https://github.com/cloud-walker) for the report.)

- **`add-to-config` action metadata now respects explicit `--config <path>` flags.** When fallow was invoked with `--config ./my-config.json`, the `auto_fixable` boolean and the `path` field on emitted `add-to-config` actions still used the auto-detected config path instead of the user-supplied one. Tools that gate auto-fix on the `auto_fixable` bool would skip a fix that was actually applicable. The path is now threaded through `IssueFilters` to the JSON-action builder so `auto_fixable` reflects whether the explicitly-named config is writable. (Closes [#367](https://github.com/fallow-rs/fallow/issues/367).)

- **The audit `--gate new-only` flow now attributes `unused-catalog-entries` and `empty-catalog-groups` findings as introduced vs inherited.** Both finding types were missing from `dead_code_keys`, `retain_introduced_dead_code`, and `annotate_dead_code_json`, so the audit verdict counted them as pre-existing baseline even when freshly added in the head commit. The keys now include line number and catalog name so a fresh entry in the same group is correctly classified as introduced; a new `audit_empty_catalog_group_changed_manifest_is_introduced` integration test locks the behavior in.
- **`docs/output-schema.json` now documents two fields the JSON output layer already emitted.** `UnresolvedImport.specifier_col` (the LSP-grade column offset of the source-string literal, used to underline just the specifier) was on the Rust struct since v2.39 but missing from the public schema, so AJV-strict consumers rejected every unresolved-import finding. `MisconfiguredDependencyOverride.target_package` (the parsed package name on `empty-value` findings) was emitted whenever the override key was syntactically valid but missing from the schema, so the value flowed through to consumers without a matching property declaration. The schema bumps these to documented + (for `specifier_col`) required so validators see the same shape as the JSON output. (Refs [#338](https://github.com/fallow-rs/fallow/issues/338))

### Internal

- **The in-scope per-finding result structs are now `#[derive(schemars::JsonSchema)]` behind a `schema` cargo feature, with a `fallow-schema-emit` dev binary that regenerates `docs/output-schema.json#/definitions` from the Rust source of truth.** A new drift test (`cargo test -p fallow-cli --features schema-emit --bin fallow-schema-emit`) verifies for every in-scope definition that the committed schema's `properties` and `required` sets match what the Rust struct derives, catching field renames / additions / required-flag drift between Rust and the public schema. `IssueAction`, `FixAction`, `SuppressLineAction`, `SuppressFileAction`, `AddToConfigAction`, and their discriminant + payload subtypes are now Rust types under `fallow_types::output` so a future PR can route `crates/cli/src/report/json.rs` through them instead of `serde_json::json!` builders. The strict structural variant of the drift gate (descriptions, integer formats, nullable union shape) is shipped as an `#[ignore]`d follow-up. (Refs [#338](https://github.com/fallow-rs/fallow/issues/338))
- **The health output subtree now derives `JsonSchema` behind a `schema` cargo feature on `fallow-cli`, so the drift gate covers `HealthFinding`, `HealthSummary`, `HealthScore` + `HealthScorePenalties`, `VitalSigns` + `VitalSignsCounts` + `RiskProfile`, `HotspotEntry` + `HotspotSummary` + `OwnershipMetrics` + `ContributorEntry`, `RefactoringTarget` + `TargetThresholds`, `HealthTrend` + `TrendCount`, `FileHealthScore`, `LargeFunctionEntry`, `CoverageGaps` + `CoverageGapSummary` + `UntestedFile` + `UntestedExport`, and the full `RuntimeCoverageReport` subtree (including the protocol-derived signal, verdict, watermark, confidence, and risk-band enums).** Per-finding action wrappers (`HealthFindingAction`, `HotspotAction`, `RefactoringTargetAction`) are typed in `fallow_types::output_health` so the action arrays attached to health findings, hotspots, and refactoring targets are now drift-checked too. Three new fields previously emitted by the JSON layer but missing from the schema (`HotspotEntry.is_test_path`, `OwnershipMetrics.suggested_reviewers`, the four `VitalSignsCounts` fields `duplicated_lines`/`total_lines`/`files_scored`/`total_deps`) are now documented. `OwnershipMetrics.unowned` drops from `required` to match the `Option<bool>` source. The `crates/cli/src/report/json.rs` emission path is unchanged; the new types are schema-only for this PR. (Refs [#338](https://github.com/fallow-rs/fallow/issues/338))

## [2.73.0] - 2026-05-13

### Fixed

- **Dependencies imported only from a config file in a hidden directory referenced by `package.json#scripts` are no longer reported as unused.** A script like `"lint": "eslint -c .config/eslint.config.js"` puts the ESLint config in `.config/`, a hidden directory the file walker normally skips. Fallow detected the file as an entry point (the script-parser found it) but never parsed it, so the imports inside (`@eslint/js`, anything else the config pulls in) became phantom unused dependencies. The walker now traverses hidden directories referenced from `package.json#scripts` config arguments (`--config` / `-c`) and positional file arguments. The same scope mechanism React Router uses for `.client` / `.server` is reused; the names are extracted per-package and scoped to the package root, so a workspace's `.config/` stays isolated from a sibling's. Three guardrails: a denylist of known build-cache / VCS / IDE directories (`.git`, `.next`, `.nuxt`, `.output`, `.svelte-kit`, `.turbo`, `.nx`, `.cache`, `.parcel-cache`, `.vercel`, `.netlify`, `.yarn`, `.pnpm-store`, `.docusaurus`, `.vscode`, `.idea`, `.fallow`, `.husky`) is never auto-scoped even when a script reads or writes into one; only the structured `config_args` / `file_args` outputs of the script parser are inspected, so a logging path like `.nx/cache/result.json` mentioned in a script body cannot pull `.nx/` into scope; and paths containing `..` segments are skipped so a workspace script referencing `../../.config/...` cannot generate a scope rooted at a sibling. Projects without hidden-dir paths in their scripts see no behavior change. Surfaces a `tracing::debug!` line per inferred scope (`RUST_LOG=fallow_core=debug`) for support diagnostics. (Closes [#358](https://github.com/fallow-rs/fallow/issues/358). Thanks [@FunctionDJ](https://github.com/FunctionDJ) for the report.)

### Added

- **`fallow fix` now auto-removes unused pnpm catalog entries from `pnpm-workspace.yaml`.** The `unused-catalog-entries` detector shipped in v2.70.0, but until now the only available action was `# fallow-ignore-next-line unused-catalog-entry`; users had to hand-edit the YAML to drop the entry. The fix is line-aware (preserves comments and stylistic choices in the file) and detects object-form entries such as `react:\n  specifier: ^18.2.0\n  publishConfig: {}` by consuming subsequent lines whose indent is strictly greater than the entry's own. When removing the last entry of a catalog group (default `catalog:` or a named `catalogs.<name>:`) leaves the header with no children, the fix rewrites the header to `catalog: {}` / `<name>: {}` so the file stays installable; bare `key:` in YAML parses as null which pnpm rejects with `Cannot convert undefined or null to object` at install time. Entries whose `hardcoded_consumers` is non-empty are skipped: removing the catalog entry while a workspace package still pins a hardcoded version of the same package would break the user's next `pnpm install`. The skip is surfaced in the human stderr summary and in the JSON output (`{"type": "remove_catalog_entry", "applied": false, "skipped": true, "skip_reason": "hardcoded_consumers", "consumers": [...], "description": "..."}`), and the per-instance `auto_fixable` bool on the check-command action correctly flips to `false` for findings with hardcoded consumers so agents that filter on the bool skip those automatically. After a successful run the CLI emits a one-line `Run \`pnpm install\` to refresh pnpm-lock.yaml` reminder so the workspace stays internally consistent. The fix output's top-level envelope adds a `"skipped"` count alongside the existing `"total_fixed"` so consumers can gate on partial-fix runs. The LSP `unused-catalog-entry` diagnostic now exposes a matching `Remove unused catalog entry` quick-fix code action with the same hardcoded-consumer guard, the same empty-parent rewrite, and an anchored key-prefix sanity check so sibling entries with shared prefixes (`react` vs `react-native`, `lodash` vs `lodash-es`) cannot be deleted by mistake. (Closes [#335](https://github.com/fallow-rs/fallow/issues/335).)

- **Detects unused and misconfigured pnpm `overrides` entries.** **Upgrade note:** the new `misconfigured-dependency-overrides` rule defaults to `error`, so a workspace with a malformed override key or empty value will flip from a green `fallow check` on v2.72 to a red one on the next minor. To absorb the change without action, set `rules.misconfigured-dependency-overrides: "warn"` in your fallow config before upgrading. Two new rules read both `pnpm-workspace.yaml`'s `overrides:` top-level (canonical, pnpm 9+) and the root `package.json`'s `pnpm.overrides` (legacy form). `unused-dependency-overrides` (default `warn`) flags entries whose target package is not declared in any workspace `package.json`; conservative static algorithm uses the parent-chain rule (`react>react-dom` is considered USED when EITHER `react` OR `react-dom` is declared, covering the CVE-fix pattern where the parent is declared and the override forces a transitive version). Findings carry the raw key, structured `target_package` / `parent_package` / `version_constraint` / `version_range` decomposition, the source file (`pnpm-workspace.yaml` or `package.json`), 1-based line number, and an optional `hint` flagging entries that may target a purely transitive dependency (CVE-fix or canary-alias pattern). `misconfigured-dependency-overrides` (default `error`) catches entries whose key cannot be parsed (empty key, dangling separators) or whose value is missing; `pnpm install` refuses to honor these. Special pnpm values (`-` removal, `$ref` self-reference, `npm:alias@^1`) are explicitly allowlisted and never flagged as misconfigured. Suppression is config-only via `ignoreDependencyOverrides: [{ package, source? }]` (inline YAML / JSON comments are not feasible since `pnpm-workspace.yaml` uses YAML and `package.json` has no comment syntax); the optional `source` field scopes a suppression to `"pnpm-workspace.yaml"` or `"package.json"`. New `IssueKind::UnusedDependencyOverride` (discriminant 23) and `IssueKind::MisconfiguredDependencyOverride` (discriminant 24); new `UnusedDependencyOverride` + `MisconfiguredDependencyOverride` structs on `AnalysisResults`. All six report formats render the findings (human two-tier, JSON with discriminated `remove-dependency-override` / `fix-dependency-override` primary actions + `ignoreDependencyOverrides` add-to-config suppress, SARIF rules `fallow/unused-dependency-override` and `fallow/misconfigured-dependency-override`, compact, markdown, CodeClimate). The GitHub Action and GitLab CI jq scripts surface both in summary tables and emit `::warning` for unused and `::error` for misconfigured annotations. The LSP emits matching diagnostics anchored on the source file line. The MCP `analyze` tool accepts `issue_types: ["unused-dependency-overrides", "misconfigured-dependency-overrides"]`, and the VS Code "Unused Code" tree shows two new categories. `fallow explain unused-dependency-override` and `fallow explain misconfigured-dependency-override` open with the CVE-pin caveat and pnpm-grammar examples respectively. (Closes [#336](https://github.com/fallow-rs/fallow/issues/336))

### Changed

- **Internal refactor:** drop redundant per-call `FileId` / path indices in detectors (`find_unused_exports`, `find_private_type_leaks`, `find_duplicate_exports`). These detectors each rebuilt the same `FxHashMap` of file lookups on every invocation; they now index `graph.modules` directly by sequential `FileId.0` (the same pattern `find_unused_files` already used). Findings counts unchanged across `next.js` (24986), `typescript`, `vite`, `vue-core`, `svelte`. No behavior change. (Closes [#333](https://github.com/fallow-rs/fallow/issues/333))
- **CI dependency bumps:** `pnpm/action-setup` 6.0.3 -> 6.0.5, `crate-ci/typos` 1.45.2 -> 1.46.0.

## [2.72.0] - 2026-05-12

### Added

- **Filter clone groups by minimum occurrence count with `duplicates.minOccurrences` (config) or `--min-occurrences N` (CLI).** Two-instance clones often reflect context-sensitive parallel code (similar reducer pairs, mirrored controller handlers) that's not worth extracting into a shared helper. Raising `minOccurrences` to `3` or higher tells fallow to report only widespread copy-paste, the kind that's actually worth refactoring. The default stays at `2` for backwards compatibility, so existing pipelines see no change on upgrade. `fallow init` now writes `minOccurrences: 3` into freshly-generated config files so new projects start with the tighter signal-to-noise ratio; existing projects can opt in by adding the key. Validation rejects `< 2` at both the config-load and CLI-parse layers with a clear message ("a single occurrence isn't a duplicate"). The filter runs AFTER line-level `// fallow-ignore-* code-duplication` suppressions, so a suppressed instance correctly reduces the group's effective size for the threshold. Threaded through `fallow dupes`, `fallow audit`, and the bare `fallow` pipeline; exposed on the MCP `find_dupes` and `trace_clone` tools; available in the napi binding (`DuplicationOptions.min_occurrences`). A new `stats.clone_groups_below_min_occurrences` JSON field discloses how many groups were hidden so consumers can lower the threshold without re-running. `duplication_percentage` keeps reflecting the FULL corpus (computed before the filter) so `threshold` gates and trend lines stay stable when the filter changes. `stats.clone_groups` and `stats.clone_instances` match the post-filter `clone_groups[]` array length. Human output emits a one-line `note: hid N clone groups below minOccurrences=M` reminder on stderr when the filter has hidden anything. (Refs [#342](https://github.com/fallow-rs/fallow/discussions/342). Thanks [@danielo515](https://github.com/danielo515) for the report.)

### Fixed

- **Workspace dependency ownership scopes per `package.json` so monorepo `unlisted-dependency` detection matches per-workspace ownership instead of the root.** Previously a dependency declared only in the root `package.json` (or any workspace's `@types/X`) globally satisfied bare imports from any workspace, hiding real `unlisted-dependency` findings inside individual workspaces. The most-specific owning workspace's `package.json` is now the source of truth for files inside that workspace; root deps only count for files outside every workspace. Upgrade impact: monorepo users may see new `unlisted-dependencies` findings where root-declared deps are imported from workspaces that do not redeclare them; the new findings are correct semantics, the prior behavior was too permissive. (Closes [#343](https://github.com/fallow-rs/fallow/pull/343). Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza) for the patch.)
- **`fallow dupes --top N` now returns the N MOST-DUPLICATED clone groups instead of N alphabetically-first ones.** The truncation step ran against a path-sorted list, so `--top 20` effectively returned twenty arbitrary groups whose file paths sorted earliest. Real-world impact on a 494-group svelte run: the previous behavior put cosmetic test-fixture pairs at positions 1 through 20 and buried the actual 33-instance hotspot at position 487. After this fix, `--top` first sorts by instance count descending (tiebreak: line count descending, then deterministic path/line) and only then truncates, so users see the widest-impact clones first. `stats.clone_groups` and `stats.clone_instances` are also recomputed to match the truncated array length so JSON consumers iterating `clone_groups[]` see the same count as the stats block; `duplication_percentage` stays corpus-wide. Within the kept set, output order remains path/line-sorted for stable rendering. CI baselines that captured `--top N` output may show different groups after upgrade.
- **VS Code extension installs on Cursor and Windsurf again by lowering the `engines.vscode` floor from `^1.116.0` to `^1.96.0`.** The 1.116 floor came in incidentally when dependabot bumped `@types/vscode`; vsce demands `engines.vscode >= @types/vscode`, so the engine floor followed. No code in the extension actually used any VS Code API newer than 1.65, so the floor was unnecessary. Windsurf 1.110.1 (and other VS Code forks tracking older bases) were rejecting install with "not compatible with Windsurf '1.110.1'". The floor now matches the December 2024 baseline that Cursor, Windsurf, and current VS Code can all satisfy. Dependabot now ignores `@types/vscode` in `editors/vscode` so the floor cannot drift again without a deliberate bump. (Refs [#287](https://github.com/fallow-rs/fallow/discussions/287). Thanks [@FunctionDJ](https://github.com/FunctionDJ) for the install-failure report.)
- **Static factory methods returning `new this()` (or `new <SameClassName>()`) no longer cause false-positive `unused-class-member` findings on the methods consumed through them.** Before the fix, `class MyClass { static getInstance() { return new this(); } getData() { return [1, 2, 3]; } }` followed by `const x = MyClass.getInstance(); x.getData();` in another file reported `MyClass.getData` as unused because the call result was untyped and fallow's instance-binding tracking only fired on direct `new ClassName()` shapes. Static methods whose body's last top-level statement is `return new this()` or `return new <SameClassName>()` now carry an `is_instance_returning_static` flag on their `MemberInfo`; a new analyze-layer pass walks `const <local> = <ID>.<method>()` bindings (cross-file via imports, same-file via local class declarations, plus barrel re-export chains) and credits accesses on `<local>` against the resolved class export. Conservative on purpose: chained `.build()` calls, ternary returns, and named-target factories (`Wrapper.getBuilder() { return new MyClass(); }`) stay out of scope to avoid wrong-class crediting. Cache version bumped from 75 to 76. (Closes [#346](https://github.com/fallow-rs/fallow/issues/346). Thanks [@robbtraister](https://github.com/robbtraister) for the report.)

### Changed

- **Routine dependency bumps for the underlying toolchain:** `napi-derive` 3.5.4 -> 3.5.5, `napi` 3.8.5 -> 3.8.6, `tokio` 1.52.1 -> 1.52.2, `rmcp` 1.5.0 -> 1.6.0, `oxc_coverage_instrument` (oxc group), `github/codeql-action` 4.35.2 -> 4.35.3. VS Code extension dev deps: `rolldown` (dev), `@tanstack/intent` (dev).

## [2.71.1] - 2026-05-12

### Fixed

- **Release workflow restores the two-step npm bootstrap so npm packages publish on Node 22 runners again.** The v2.71.0 release workflow consolidated the npm install to a single `npm install -g --ignore-scripts npm@11.14.1` step, but the Node 22 runner's bundled npm has a broken dependency tree (`Cannot find module 'promise-retry'`) when asked to install npm@11.x directly; the v2.71.0 `Publish to npm` job failed before any tarball reached the registry, leaving `npm install fallow` at v2.70.0 while crates.io, the GitHub release, and the VS Code marketplace all shipped v2.71.0. The install step now does `npm install -g --ignore-scripts npm@10.9.8` first to repair the tree, then upgrades to the pinned `npm@11.14.1` that holds the OIDC trusted-publishing config. Both invocations keep `--ignore-scripts` so the supply-chain hardening surface stays narrow. v2.71.0 is functionally equivalent on every platform except npm; users on `npm` jump from `2.70.0` directly to `2.71.1`.

## [2.71.0] - 2026-05-12

### Added

- **`import type { CheckOutput, FallowJsonOutput } from "fallow/types"` now works for any TypeScript consumer of `npx fallow --format json`.** Previously CI scripts, monorepo tooling, and AI agents parsing fallow output invented their own `interface`s or used `any`. The npm `fallow` package now exposes the generated `.d.ts` the VS Code extension already uses, covering every command's JSON shape (`CheckOutput`, `HealthOutput`, `DupesOutput`, `AuditOutput`, `CombinedOutput`, `IssueAction`, every per-finding struct). `SchemaVersion` is pinned to a literal (currently `6`) at codegen time, so a future major-version schema bump produces a compile error at consumer call sites that gate on the version rather than silently drifting. Works on both modern (`moduleResolution: "node16"` / `"nodenext"` / `"bundler"`) and legacy (`"node"`) TypeScript configs via paired `exports` and `typesVersions` entries. Types-only, no JS runtime added. (Closes [#339](https://github.com/fallow-rs/fallow/issues/339))
- **Detects pnpm catalog references whose catalog does not declare the package.** **Upgrade note:** the new rule's default severity is `error`, so a workspace that already had a broken catalog reference will flip from a green `fallow check` on v2.70 to a red one on the next minor. To absorb the change without action, set `rules.unresolved-catalog-references: "warn"` in your fallow config before upgrading; pin to `"error"` later once the broken references are resolved. New `unresolved-catalog-references` rule (default severity `error`) walks every workspace `package.json` and flags `catalog:` / `catalog:<name>` references whose target catalog has no entry for the consumed package. `pnpm install` fails with `ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_CATALOG_PROTOCOL` on these references; fallow surfaces them statically so the failure is caught at `fallow check` time, before any install. Each finding carries the consumer `package.json` path + line, the broken `(package, catalog)` pair, and an `available_in_catalogs` list naming OTHER catalogs in the same workspace that DO declare the package; agents and humans can flip the reference to one of those catalogs instead of growing a new entry. The default catalog renders with a special-case "default catalog" phrasing across human / JSON / SARIF / markdown / CodeClimate / LSP output. Suppression is config-only because `package.json` has no comment syntax: the new `ignoreCatalogReferences: [{ package, catalog?, consumer? }]` array suppresses by package name, optionally scoped to a specific catalog and / or consumer-path glob (covers staged catalog migrations where the catalog edit lands separately, and library-internal placeholder packages whose target catalog isn't ready yet). JSON `actions[]` is discriminated: when `available_in_catalogs` is non-empty the primary action is `update-catalog-reference` (high-confidence machine-actionable fix), otherwise `add-catalog-entry`; `remove-catalog-reference` is the fallback, and the `add-to-config` suppress action carries a paste-ready `ignoreCatalogReferences` value with a `value_schema` pointer. New `IssueKind::UnresolvedCatalogReference` (discriminant 22, suppression tokens `unresolved-catalog-reference` and `unresolved-catalog-references`); new `UnresolvedCatalogReference` finding on `AnalysisResults`. The shared YAML parse + workspace `package.json` walk is reused with the existing `unused-catalog-entries` detector so both rules cost a single pass. All six report formats render the finding (human two-tier + default-catalog special case, JSON, SARIF rule `fallow/unresolved-catalog-reference`, compact, markdown, CodeClimate), the GitHub Action and GitLab CI jq scripts surface it in summary tables and emit `::error` annotations on the consumer `package.json` line, the LSP emits an `ERROR`-severity `unresolved-catalog-reference` diagnostic anchored on the consumer `package.json` line, the MCP `analyze` tool accepts `issue_types: ["unresolved-catalog-references"]`, and the VS Code "Unused Code" tree shows a new category with an error icon. `fallow explain unresolved-catalog-reference` opens with the install-failure consequence and cross-references the inverse `unused-catalog-entries` rule (both directions). (Closes [#334](https://github.com/fallow-rs/fallow/issues/334))

### Fixed

- **Combined, audit, and `flags` JSON output now emit `schema_version: 6` (was `3`), matching every other command and the documented `SchemaVersion` definition.** Three top-level envelope writers (`crates/cli/src/combined.rs`, `crates/cli/src/audit.rs`, `crates/cli/src/flags.rs`) hardcoded `schema_version: 3` independently of the central `SCHEMA_VERSION` constant in `crates/cli/src/report/json.rs`, so the bare `fallow --format json` invocation, `fallow audit --format json`, and `fallow flags --format json` all stayed pinned at version 3 while `fallow check`, `fallow health`, `fallow dupes`, etc. progressed through `4`, `5`, and `6`. The schema at `docs/output-schema.json` and the generated `fallow/types` `SchemaVersion` literal both reference the central constant, so consumers gating on `schema_version === 6` saw runtime `3` from any of the three envelopes. All three sites now read from `report::SCHEMA_VERSION` directly, eliminating the drift class. Surfaced by an end-to-end probe of the new `fallow/types` subpath: the literal-pinned types matched per-command outputs but not the combined envelope. No `--format json` schema content changed; only the metadata field's value harmonized upward.

### Changed

- **VS Code extension TypeScript types for fallow's JSON output are now generated from `docs/output-schema.json` instead of hand-maintained.** The hand-written `editors/vscode/src/types.ts` shapes have repeatedly drifted from the Rust source of truth: issue [#323](https://github.com/fallow-rs/fallow/issues/323) crashed the Unused Code tree because `UnlistedDependency` had been restructured Rust-side but the TS interface still read the removed `path` field, and an audit at the time surfaced two more latent drifts on `UnusedDependency` and `TypeOnlyDependency`. The contract is now derived: `editors/vscode/scripts/codegen-types.mjs` runs `json-schema-to-typescript` against `docs/output-schema.json` and writes `editors/vscode/src/generated/output-contract.d.ts` (committed). `prebuild` and `prepackage` regenerate before the bundle is built or packaged, so the marketplace artifact never ships against stale types. CI runs `pnpm run check:codegen` as a dedicated step so a forgotten regen fails the build with an actionable message. Existing `FallowCheckResult` / `FallowDupesResult` / `FallowCombinedResult` aliases stay re-exported from `types.ts` so consumer code keeps compiling; new code should prefer the schema-derived names (`CheckOutput`, `DupesOutput`, `CombinedOutput`). The settings shapes that are NOT in the output contract (`IssueTypeConfig`, `DuplicationMode`, `TraceLevel`), UI label maps (`IssueCategory`, `ISSUE_CATEGORY_LABELS`), and the still-unschematized `fallow fix --format json` shape (`FixAction`, `FallowFixResult`) split out into `src/settings.ts`, `src/labels.ts`, and `src/fix-types.ts` and stay hand-written. Renamed the `Action` definition in `docs/output-schema.json` to `IssueAction` so the generated TS does not collide with the consumer-facing `FixAction` (fallow-fix output entry); no `schema_version` bump because the rename is to an internal definition name and does not change the JSON payload. (Closes [#326](https://github.com/fallow-rs/fallow/issues/326))

## [2.70.0] - 2026-05-12

### Added

- **Runtime coverage Phase 2 surfaces a `hot-path-touched` verdict in `fallow audit` when a PR diff overlaps a hot path.** Consumes `fallow-cov-protocol 0.5` which gains a per-hot-path `end_line` so the sidecar reports the full span of every hot path instead of just its start. The new `--diff-file <unified.diff>` flag on `fallow audit` (and the matching `diff-file` input on the GitHub Action / GitLab CI template) feeds the PR's unified diff into the runtime-coverage matcher, which intersects the changed line ranges against each hot path's `[start_line, end_line]` and fires the verdict when any overlap is found. The GitHub Action and GitLab CI templates pre-compute the diff in the analyze step so the verdict fires automatically on PRs; PR/MR comments render a "hot path touched" framing when the verdict fires (separate from the existing `hot-path-changes-needed` block) so reviewers see at a glance which changed lines landed on production-hot code. Protocol JSON gains a structured `signals[]` array (replacing the prior flat `signal` string) for forward-compatible multi-signal verdicts; PR-aware precedence ordering picks the strongest applicable verdict when multiple fire. Internal: `HotPathChangesNeeded` renamed to `HotPathTouched` across the runtime-coverage layer; filter context lifted into a dedicated struct.
- **VS Code "Unused Code" tree surfaces four additional diagnostic categories.** Previously the tree only rendered the original five categories (unused files / exports / types / enum members / class members); namespace-barrel duplicate exports, circular dependencies, boundary violations, and unused-catalog-entry findings all existed in fallow's JSON output but never showed up in the sidebar. The tree builder now branches on `IssueKind` and renders each category under its own collapsible header with the same path-relative line/column display the existing categories use, so users get a complete picture of what fallow found without dropping to the terminal.
- **`add-to-config` actions now carry an optional `value_schema` URL pointing at the schema fragment that describes the `value` shape.** AI agents and other programmatic consumers can fetch the linked schema and validate the action's `value` before writing into a user's config. The URL is a JSON Pointer fragment into fallow's `schema.json` (`#/properties/ignoreExports` for the `duplicate-exports` action, `#/properties/ignoreDependencies/items` for the `ignoreDependencies` action emitted on dependency findings). Strictly additive optional field, no `schema_version` bump; consumers that ignore the field keep working unchanged. Documented in `docs/output-schema.json` under `AddToConfigAction.value_schema`. (Closes [#331](https://github.com/fallow-rs/fallow/issues/331))
- **Detects unused entries in pnpm `catalog:` / `catalogs:` sections.** New `unused-catalog-entries` rule (default severity `warn`) reads `pnpm-workspace.yaml`, parses the default `catalog:` map plus every named catalog under `catalogs:`, and reports entries that no workspace `package.json` references via the `catalog:` protocol. Both `"react": "catalog:"` (bare, default catalog) and `"react": "catalog:default"` are treated as references to the default catalog per pnpm's spec. Each finding carries the entry name, catalog name, source line, and a `hardcoded_consumers` list pointing at workspace `package.json` files that declare the same package with a hardcoded version range instead of `catalog:`; surfacing the latter lets AI agents and humans choose between "switch consumers to `catalog:`" and "delete the catalog entry" instead of rubber-stamping the wrong one. New `IssueKind::PnpmCatalogEntry` (discriminant 21, suppression tokens `unused-catalog-entry` and `unused-catalog-entries`); new `UnusedCatalogEntry { entry_name, catalog_name, path, line, hardcoded_consumers }` on `AnalysisResults`. All six report formats render the finding (human / JSON / SARIF / compact / markdown / CodeClimate), the GitHub Action and GitLab CI jq scripts surface it in summary tables and annotations, the LSP emits a `unused-catalog-entry` diagnostic anchored on the YAML line, the MCP `analyze` tool accepts `issue_types: ["unused-catalog-entries"]`, the VS Code "Unused Code" tree shows a new category, and JSON `actions[]` injects a YAML-style `# fallow-ignore-next-line unused-catalog-entry` suppress action. Thanks [@Luzefiru](https://github.com/Luzefiru) for the report. (Closes [#329](https://github.com/fallow-rs/fallow/issues/329))
- **Human `Duplicate exports` section surfaces a namespace-barrel orientation hint when the findings look like a shadcn / Radix-clone barrel layout.** The JSON `remove-duplicate` action has carried a `note` calling out the namespace-barrel shape since #317 ("If every location is the sole `index.*` of its directory, this is likely an intentional namespace-barrel API. Prefer the `add-to-config` ignoreExports action over removing exports."), but `--format human` never showed it, so users seeing 40+ duplicate-exports findings in a shadcn-style codebase had no surfacing of the recommended escape hatch. The human renderer now emits the same string once at the bottom of the section when at least 80% of renderable findings have every location matching `**/<dir>/index.{ts,tsx,js,jsx,mjs,cjs}` (case-insensitive on the extension only) AND there are at least 3 renderable findings. The wording is now a shared `NAMESPACE_BARREL_HINT` constant so the human and JSON output paths cannot drift. The dual gate prevents the hint from spamming small projects with one or two coincidental duplicates while still firing reliably on the shadcn / Radix-clone case where dozens of `components/ui/<name>/index.ts` barrels intentionally share short export names. (Refs [#322](https://github.com/fallow-rs/fallow/issues/322))

### Changed

- **Workspace and plugin discovery now run in parallel hot loops.** The first cut walked workspace `package.json` files serially and ran every plugin's `extract_referenced_dependencies` pass sequentially per project; with rayon already on the dependency tree the serial paths were leaving cores idle on large monorepos. Both passes now distribute over a rayon thread pool with results merged back through the existing aggregator; the parallel build is benchmark-stable across the eight workspace fixtures (svelte, vue-core, fastify, query, vite, preact, zod, next.js) with no regressions on smaller projects (the rayon scheduler's work-stealing keeps overhead at zero on single-package layouts).
- **Dead-code analysis early-skips modules with no exports and lazy-builds detector matcher lists.** Two micro-passes that together cut a measurable slice off `fallow check` on monorepos with many ambient `.d.ts` / barrel-less utility files: the unused-exports detector now bails before the matcher walk when the file has zero export records, and the matcher lists for `unused-types` / `unused-exports` / `unused-class-members` are built lazily per-file on first finding instead of upfront for every file. No behavior change; same findings, less work per file.
- **Duplicate-clone detection caches per-finding suppression metadata.** `find_duplicates` was re-parsing each clone instance's source file for fallow-ignore directives once per overlapping clone group; long files with many overlapping clones (the shadcn / Radix component-library shape) burned proportional time on redundant scans. The suppression metadata for each `(file, line-range)` pair is now memoised within a single `find_duplicates` invocation, so the second-and-later groups touching the same file read from a cache instead of re-parsing. No JSON, CLI, or cache-format change.
- **`overrides.rules.{duplicate-exports,circular-dependency}` load-time warnings dedupe to one per `(rule, glob list)` per process in workspace mode.** When a top-level config sets `overrides.rules.duplicate-exports` or `overrides.rules.circular-dependency` for a file glob, every workspace package resolve was re-emitting the same `tracing::warn` ("`overrides.rules.duplicate-exports has no effect for files matching [...]`"). On a 200-package monorepo with a single top-level config, that surfaced as 200 (or 400 with both rules set) identical warn lines per invocation, training users to filter `tracing` output entirely. The warning is now deduplicated against a process-local set keyed on a stable hash of `(rule name, sorted glob list)`, so logically identical override blocks across packages produce a single warning per process lifetime. The set persists across resolves within a single process (so `fallow watch`, the LSP server, and NAPI consumers re-using a worker see the warning once until the host restarts) and is documented as such in the resolver source. Internal pre-compilation of `ignoreExports` globs also moves to `ResolvedConfig` construction, so the existing "invalid `ignoreExports` pattern" warning fires at resolve time once instead of once per detector invocation, and both `find_unused_exports` and `find_duplicate_exports` read pre-built matchers from the shared compiled list. Policy note (ADR-008): `fallow-core`'s `pub fn analyze::*` items are explicitly internal; embedders should consume `fallow_cli::programmatic` (a deprecation cycle for direct `fallow-core` consumers will land in a subsequent minor release). (Refs [#322](https://github.com/fallow-rs/fallow/issues/322))

### Fixed

- **Wildcard tsconfig paths (`"*": ["./src/*"]`) no longer make Node / Bun / Cloudflare builtin imports surface as `unresolved-import`.** `import { fileURLToPath } from "node:url"` was reported as unresolved whenever the project's `tsconfig.json` defined a wildcard `paths` entry, even with `@types/node` installed and listed under `compilerOptions.types`. The TypeScript plugin's `paths` normalization strips the trailing `*` from each find pattern; for the wildcard `"*"` that produced an empty-prefix entry in `path_aliases`, and the resolver's `matches_plugin_alias` fast path (`specifier.starts_with(prefix)`) then matched every specifier (since every string starts with `""`) and routed them through the path-alias fallback, which classifies failed-alias resolutions as `Unresolvable`. Wildcards are still honoured by `oxc_resolver`'s native tsconfig paths handling, so dropping them from `path_aliases` does not regress legitimate `*` rewrites (e.g. `import { greeting } from "helpers"` still resolves to `./src/helpers.ts`). Side effect of the same root cause: bare-specifier typos (`import { x } from "doesnotexist"`) in wildcard-paths projects were also misreported as `unresolved-import` instead of `unlisted-dependency`; the fix restores parity with the no-paths baseline so the messaging matches. Thanks [@FunctionDJ](https://github.com/FunctionDJ) for the report and the minimal reproduction repo. (Closes [#327](https://github.com/fallow-rs/fallow/issues/327))
- **Cross-package namespace-object aliases stay credited when the alias target itself does `export * as N from './source'`.** Follow-up to #303 and #310. The first-cut Phase 2b pass credited consumer accesses of the form `<consumer_local>.<suffix>.<member>` onto the namespace target file. Real-world chains layer a second indirection: the namespace target is itself a barrel whose only purpose is to namespace-re-export deeper files (`export * as inner from './leaf'`). For a consumer access `<consumer_local>.<suffix>.<inner>.<X>`, Phase 2b credited `inner` on the barrel (so the synthesised `inner` stub got a reference) but stopped there; Phase 4's named-re-export propagation never follows `imported_name="*"` edges, so the underlying `X` on `./leaf` stayed flagged as a false `unused-export`. The Phase 2b matcher now walks chained `export * as <name> from './source'` re-exports on the alias-target side after the initial credit: when the credited member name lands on a namespace re-export edge, the consumer's deeper accesses (`<expected_object>.<member>.<X>`) are propagated onto the underlying source recursively. Cycle-protected by a per-access visited set keyed on module index. Per-member precision is preserved (same-file unused siblings stay flagged), per-re-export-name precision is preserved (sibling namespace re-exports the consumer never touches stay flagged), and multi-hop chains (`export * as outer from './deeper-barrel'` followed by `export * as deep from './deeper-leaf'`) credit all the way through. No JSON, CLI, or cache-format change. Thanks [@filipw01](https://github.com/filipw01) for the report and the precise reproduction repo. (Closes [#328](https://github.com/fallow-rs/fallow/issues/328))
- **`export * as Foo from './bar'` followed by `import { Foo } from './barrel'; Foo.X(...)` no longer falsely flags `X` as `unused-export`.** A barrel that namespace-re-exports a module records a `ReExportEdge { imported_name: "*", exported_name: "Foo" }` plus a synthesised `ExportSymbol` named `Foo`. The Phase 2 reference-narrowing pass only narrowed member accesses for `import * as ns` (the direct namespace import form), so a consumer using the named import form for the same logical namespace got no member credit on the target file. Phase 4's named-re-export propagation then looked for a source export literally named `"*"` and matched nothing, so every export of the re-exported source file stayed unreferenced. A new Phase 2c pass (`namespace_re_exports`) walks every namespace re-export edge, enumerates the consumer files that import the re-exported name directly or through outer named-re-export barrels, collects each consumer's `<local>.<member>` accesses and whole-object uses, and credits the matching members on the namespace target file via the same `mark_member_exports_referenced` plus `create_synthetic_exports_for_star_re_exports` pair used for direct namespace imports. Multi-hop named-re-export chains (`outer -> inner -> source`) are handled by a forward BFS on re-export edges; `export * from` propagates the source name unchanged; renamed re-exports (`export { Foo as Bar }`) follow the new name; barrels that expose the namespace through an entry point credit every target export to match the existing `propagate_entry_point_star` semantics. Real-world delta on the `astro` benchmark fixture: 103 false-positive `unused-export` findings (every `AstroErrorData.<ErrorName>` entry in `packages/astro/src/core/errors/errors-data.ts`) disappear. No JSON, CLI, or cache-format change. Thanks [@ivayloc](https://github.com/ivayloc) for the report and the precise reproduction. (Closes [#324](https://github.com/fallow-rs/fallow/issues/324))
- **VS Code: tree view no longer crashes with `The "path" argument must be of type string. Received undefined` when an unlisted dependency is present.** The extension's TypeScript type for `UnlistedDependency` had drifted from the Rust struct: it declared a single top-level `path` field but the actual JSON shape carries `imported_from: ImportSite[]` (so one unlisted dependency reports every site that imports it, with per-site file / line / column). When the user's project produced at least one unlisted dependency, the tree view's `unlisted-dependencies` mapping passed `d.path` (`undefined`) into Node's `path.isAbsolute`, which threw and prevented the tree from rendering, so the "Unused Code" tab silently fell back to the empty "No unused code found" welcome message. The fix updates the TypeScript type to match the Rust shape, mirrors the existing `duplicate_exports.locations.flatMap` pattern to emit one tree row per import site (so each row navigates to the actual import line), and hardens `resolveFilePath` to return empty strings on undefined / empty input as a defensive guard against any future shape drift. The audit also surfaced two latent type drifts in the same file that hadn't crashed yet only because no consumer happened to read the missing fields: `UnusedDependency` now declares the `location` (`"dependencies" | "devDependencies" | "optionalDependencies"`), `line`, and optional `used_in_workspaces` fields that the Rust struct has always emitted, and `TypeOnlyDependency` now declares the `line` field. Thanks [@FunctionDJ](https://github.com/FunctionDJ) for the report. (Closes [#323](https://github.com/fallow-rs/fallow/issues/323))
- **`ignoreExports` now also excludes matching files from `duplicate-exports` grouping, and `overrides.rules.duplicate-exports` emits a load-time warning instead of silently no-opping.** shadcn / Radix / bits-ui-style component libraries ship a folder per component (`components/ui/<name>/index.ts`) where every barrel intentionally exports the same short names (`Root`, `Content`, `Trigger`, `Title`, ...) so consumers can do `import * as Dialog from '$lib/components/ui/dialog'`. Without this fix, every short name surfaced as a `duplicate-export` group across the whole component set even though the duplication is the API. `ignoreExports` was the obvious escape hatch but only gated `unused-export` findings; `overrides.rules.duplicate-exports = "off"` on a file glob silently did nothing because the rule's severity is project-wide and the finding spans multiple files. The fix: a file matched by `ignoreExports` is excluded from the duplicate-exports grouping at the file level (`exports: ["*"]` removes the file entirely, a name list removes only those names), so the documented config knob works the way the field name implies. Setting `overrides.rules.duplicate-exports` or `overrides.rules.circular-dependency` now emits a `tracing::warn` at config load pointing users at the working escape hatch (`ignoreExports` for duplicates, `// fallow-ignore-file circular-dependency` for cycles), so the silent no-op is gone. JSON `duplicate_exports` findings gain a new `add-to-config` action with a paste-ready `ignoreExports` snippet built from the finding's actual location paths, and the action is emitted in position 0 (the documented primary slot) so AI agents that pick the first action get the non-destructive path for the namespace-barrel case rather than being nudged to delete one of the duplicates. The note on the existing `remove-duplicate` action now calls out that locations shaped like `<name>/index.*` are likely intentional namespace barrels and should prefer the config action. JSON `schema_version` bumps to `6` because `AddToConfigAction.value` widens from a scalar string to `oneOf: [string, array]` so the `ignoreExports` action can carry an array of `{ file, exports }` rule objects; the legacy `ignoreDependencies` / `type-only-dependency` / etc. variants still emit strings, so consumers that switch on `config_key` keep working unchanged. Real-world delta on `huntabyte/shadcn-svelte`'s `docs` package with `ignoreExports: [{ "file": "src/lib/registry/ui/**", "exports": ["*"] }]`: 29 duplicate-export groups drop to 3 (the 26 that flipped off are exactly the shadcn barrel names `Root`, `Content`, `Trigger`, `Dialog`, ...; the 3 remaining are genuine duplicates in non-registry files). Thanks [@Vantrongs](https://github.com/Vantrongs) for the report and the precise reproduction. (Closes [#317](https://github.com/fallow-rs/fallow/issues/317))
- **`// fallow-ignore-next-line` accepts multiple kinds stacked on consecutive lines.** The single-comma `// fallow-ignore-next-line unused-export, unused-type` form has always worked, but stacking the same suppression as two adjacent single-kind comments (`// fallow-ignore-next-line unused-export` then `// fallow-ignore-next-line unused-type`, both above the same target line) only honoured the comment closest to the target and silently dropped the other. The comment scanner now coalesces every contiguous `fallow-ignore-next-line` comment block above a target line and unions their kinds, so the documented stack-or-comma equivalence holds in both directions. Doc note added to the suppression reference to make the equivalence explicit. (Closes [#318](https://github.com/fallow-rs/fallow/issues/318))
- **Duplicate-clone detector filters out atomic invocation-shaped clones (bare function calls).** A 5-line call expression like `someFunction(\n  arg1,\n  arg2,\n  arg3,\n)` is already a single named abstraction; flagging it as a duplicate clone produces no actionable extraction (there is nothing to factor out beyond the call itself). The detector now identifies clones whose normalised token stream is a single invocation expression (`<callee>(args...)` with no surrounding statements) and drops them before grouping. Multi-statement clones that happen to contain a call as one of their statements are preserved; only the bare-call shape is filtered. Closes the false-positive class reported in discussion [#320](https://github.com/fallow-rs/fallow/discussions/320). Thanks [@danielo515](https://github.com/danielo515) for the report.
- **Singular rule keys in `.fallowrc` are accepted as aliases for every plural rule.** The canonical rule keys describe categories and are plural (`unused-exports`, `unused-types`, `boundary-violations`, `private-type-leaks`, ...) but the matching `IssueKind` names are singular (`unused-export`, `unused-type`, `boundary-violation`, `private-type-leak`). Users writing config from memory of the issue type instead of the rule reference saw their `"boundary-violation": "warn"` silently ignored (the schema accepted it but no rule matched it). Every plural rule key now also accepts its singular form via a serde alias, so the rule fires either way. The human-mode footer now uses the singular `boundary-violation` token when nudging users to add the rule to their config, matching the canonical issue-kind spelling used in suppression comments and JSON output. The full alias table is documented in the rules reference.
- **Rustdoc intra-doc link `[`RuntimeCoverageReport::signals`]` uses the fully-qualified path so `cargo doc --document-private-items` stays green on CI.**

### Security

- **Drop the unmaintained `serde_yml 0.0.12` crate (RUSTSEC-2025-0068, unsound segfault advisory) in favour of `serde_yaml_ng 0.10`.** `serde_yml` was archived after unsoundness issues were raised against its YAML serializer (`Serializer.emitter` segfault, RUSTSEC-2025-0068) and `cargo deny check advisories` started rejecting it. Both crates expose the same `Value` / `Mapping` / `from_str` API used by the pnpm catalog parser, so the swap is a one-line `use` rename plus dep update. No behavior change; the parser handles the same set of `pnpm-workspace.yaml` shapes as before, and the 14 catalog parsing tests continue to pass without modification.

## [2.69.0] - 2026-05-09

### Added

- **VS Code: `fallow.configPath` setting mirrors the CLI's `--config` flag.** A new string setting points the extension at an explicit Fallow config file. Absolute paths pass through; relative paths are resolved against the workspace root (the first folder in multi-root workspaces) before being forwarded to both the LSP via `initializationOptions.configPath` and the CLI via `--config <path>` for analyze and fix commands. Empty (the default) keeps the existing auto-discovery behavior (`.fallowrc.json` > `.fallowrc.jsonc` > `fallow.toml` > `.fallow.toml`). Changing the setting restarts the LSP and re-runs the sidebar analysis. A broken `configPath` (typo, missing file, parse error) surfaces a WARNING-level notification including the path so users see what went wrong instead of a silent empty Problems panel; auto-discovery still falls back to defaults on parse errors. Two new public `fallow_core` entry points (`config_for_project` and `analyze_project_with_config`) let other programmatic embedders request an explicit config the same way. Thanks [@FunctionDJ](https://github.com/FunctionDJ) for the question. (Closes [#314](https://github.com/fallow-rs/fallow/discussions/314))

## [2.68.0] - 2026-05-09

### Added

- **Typed PR / MR comment and review-envelope formats replace the bash + jq feedback layer.** Four new `--format` variants render the surfaces the GitHub Action and GitLab CI templates post on every pull / merge request: `pr-comment-github` and `pr-comment-gitlab` produce sticky comment markdown with a `<!-- fallow-id: ... -->` HTML-comment marker for upsert, and `review-github` and `review-gitlab` produce JSON envelopes shaped for the respective inline-review POST endpoints (GitHub PR Reviews API, GitLab Discussions API) with deterministic per-finding fingerprints, side-aware positions, and code-suggestion blocks for unused-export / unused-class-member / unused-enum-member / unused-files findings. Combined mode renders a single sticky comment with collapsible `<details>` sections per category (Dead code, Dependencies, Duplication, Health, Architecture, Suppressions); truncated sections foreshadow the cap in the section header (`Duplication (160, showing 50)`). The companion `fallow ci reconcile-review --provider {github,gitlab}` subcommand reads a typed review envelope, joins it against existing review threads, posts an idempotent `Resolved in <sha>` reply per stale finding, and resolves the thread; bot-authorship gated, rate-limit-aware retry capped at 60s per attempt with bounded total budget. Eleven new typed `inputs:` / `variables:` (`comment-id`, `diff-filter`, `max-comments`, `api-retries`, `api-retry-delay`, ...) make every wrapper knob discoverable in the GitHub Marketplace UI and the GitLab template instead of being env-var-only. `--workspace <one>` auto-suffixes the sticky marker with the workspace name; `--workspace web,admin` hashes the sorted list to a stable 6-hex suffix so parallel monorepo jobs do not race each other's sticky body on the same PR. The CodeClimate format gains a deterministic `fingerprint` (required by the GitLab Code Quality spec) and SARIF gains `partialFingerprints` under both `tools.fallow.fingerprint/v1` and `primaryLocationLineHash/v1` so GitHub Code Scanning's alert-correlation engine deduplicates fallow alerts across pushes. Twelve review-pad jq files retire; `comment.sh` and `review.sh` shrink to typed-format wrappers with bounded `gh_api_retry` / `curl_retry` helpers, paginated comment-list lookups, and explicit structured-error envelope guards. Suggestion blocks widen to enum-members, class-members, and a text hint for unused-files (no `\`\`\`suggestion` block since deletion needs a separate review action). (#315)
- **`fallow migrate` writes `.fallowrc.jsonc` when migrating from a `.jsonc`-named source, and a new `--jsonc` flag forces it explicitly.** `fallow migrate` has always generated JSONC content (with `//` comments) but wrote it to `.fallowrc.json`, which trips up editor JSON validators and project lint rules that flag comments in `.json`. The output filename is now picked in this order: `--toml` writes `fallow.toml`, `--jsonc` writes `.fallowrc.jsonc`, otherwise the source extension is mirrored, so a `knip.jsonc` or `.knip.jsonc` migration writes `.fallowrc.jsonc` and a `knip.json` / `package.json` `knip` key migration writes `.fallowrc.json`. `--jsonc` and `--toml` are mutually exclusive (clap rejects them together). The generated content is identical between the two JSON variants; the `.jsonc` extension exists so editors auto-detect JSON-with-comments syntax highlighting.

### Fixed

- **`.fallowrc.jsonc` is consistently documented across every user-facing surface.** The config crate has detected `.fallowrc.jsonc` since v2.57.0 (it sits second in the canonical priority order, mirroring `tsconfig.json` > `tsconfig.jsonc`), but the supporting documentation drifted: the MCP `config` param doc, the `fallow --config` flag help, the GitHub Action `config` input description, the GitLab CI `FALLOW_CONFIG` comment, the VS Code first-run "configure" page, the `AGENTS.md` configuration paragraph, two `fallow-docs` page meta descriptions (`configuration/overview`, `cli/init`), and the bundled npm-vendored `cli-reference.md` all listed only a subset (`.fallowrc.json` plus `fallow.toml`, sometimes omitting both `.fallowrc.jsonc` and `.fallow.toml`). Each surface now lists the full `.fallowrc.json` > `.fallowrc.jsonc` > `fallow.toml` > `.fallow.toml` set with a one-line note that the two JSON variants share the same JSONC parser and the `.jsonc` extension exists so editors auto-detect JSON-with-comments syntax highlighting. No behavior change: detection has always worked. Thanks [@pdeveltere](https://github.com/pdeveltere) for the report. (Closes [#313](https://github.com/fallow-rs/fallow/issues/313))
- **`vi.mock('./target', factory)` credits the target file and no longer surfaces a phantom `__mocks__/target` as an `unresolved-import`.** Two bugs fell out of one: vitest only consults the `__mocks__/<file>` sibling when no factory is provided, but fallow's auto-mock heuristic synthesized a `__mocks__/<file>` import for every `vi.mock` call regardless of factory. With a factory present, the synthesized sibling almost never exists, so it surfaced as a false `unresolved-import`; meanwhile the actual target (the file the user wrote in the first argument) was never recorded as a referenced module, so the target file became a false `unused-file`. The visitor now records the vi.mock target itself as side-effect reachability, and only synthesizes the `__mocks__/<file>` sibling when no factory is provided. This keeps the target file reachable without blanket-crediting every export in it. Detects arrow factories, function-expression factories, and nested parenthesized variants (`vi.mock('x', (((() => ({})))))`); preserves the existing options-form behavior (`vi.mock(spec, { spy: true })` still synthesizes because vitest still consults `__mocks__/<file>` in that shape). Bumps the extract `CACHE_VERSION` so warm caches do not retain pre-fix data. Thanks [@filipw01](https://github.com/filipw01) for the report and the minimal reproduction repo. (Closes [#311](https://github.com/fallow-rs/fallow/issues/311))
- **Cross-package namespace-object aliases stay credited through multi-hop named-re-export barrels.** Follow-up to #303. The first-cut Phase 2b pass only credited consumers whose `import { API } from '@scope/lib'` resolved DIRECTLY to the file containing `import * as bar from './bar'; export const API = { bar }`. Real-world workspace packages layer 1-3 named-re-export barrels between the package's public entry and the alias-defining file (e.g. `apps/foo/src/foo.ts` -> `packages/bar/src/index.ts` -> `packages/bar/src/methods/index.ts` -> `packages/bar/src/methods/methods.ts`), and consumers landing at any intermediate barrel were missed, surfacing `searchFoo` (and every other `API.bar.<member>` access target) as a false `unused-export`. The Phase 2b matcher now does a forward BFS over the graph's re-export edges starting from `(alias_file, via_export_name)` to enumerate every `(barrel_file, exported_name_at_barrel)` pair through which the alias is reachable, then matches consumer imports against the full set. Plain `export * from './src'` propagates names unchanged; `export { A as B } from './src'` propagates the renamed `B`; `export * as ns from './src'` is correctly skipped so unrelated same-named exports of the source are never spuriously credited at a namespace-aliased star barrel. Cycle-protected via the visited set. Thanks [@filipw01](https://github.com/filipw01) for the report and the precise multi-hop reproduction repo. (Closes [#310](https://github.com/fallow-rs/fallow/issues/310))
- **Angular `@if (member(); as alias)` no longer flags the conditioned member as unused.** Fallow previously reported a false-positive `unused-class-member` when an Angular 17+ `@if` block bound its truthy result to a template-local via `; as alias`. The Angular template scanner passed the entire parenthesized content (`withAlias(); as aliased`) to oxc as a single expression, which oxc rejects (a `;` inside `void (...)` is invalid), so neither the call nor the alias was extracted. The scanner now splits the alias clause off the condition before parsing, parses the condition normally, and registers the alias as a block-scoped local so `{{ alias }}` inside the body does not leak as an unresolved identifier. Applies to inline `@Component({ template: \`...\` })` and external `templateUrl` shapes, and to nested `;` inside string literals or call arguments. Bumps `CACHE_VERSION` so warm caches do not retain pre-fix data. Thanks [@OmerGronich](https://github.com/OmerGronich) for the report and the precise reproduction. (Closes [#308](https://github.com/fallow-rs/fallow/issues/308))
- **PR / MR comment Complexity dropdown header pluralizes when only one function is above threshold.** The `<details>` summary previously read `(1 functions above threshold)` for a single-finding PR. Each variant now branches on `count == 1` to produce `(1 function above threshold)`. Same fix applied to the GitHub Action and GitLab CI variants of `summary-combined.jq`.
- **Inline review-comment "Also found in:" links keep the full path in the URL when display is shortened.** Same `rel_path` truncation bug as the dupes-locations table fix: the `file_link` helper in `review-comments-dupes.jq` (both Action and GitLab variants) used the truncated path for both display and URL, producing 404 links on monorepo paths longer than 3 segments. The helper now applies `rel_path` only to the display half and uses the caller-supplied full path in the URL. Regression test covers a 6-segment monorepo path.
- **PR / MR comment Duplication links keep the full path in the URL when display is shortened.** The new clone-locations table truncates display paths to the last 3 segments for compactness (e.g. `services/billing/calculator.ts` instead of `apps/web/src/services/billing/calculator.ts`), but the previous version used the truncated path for BOTH display AND the GitHub / GitLab blob URL. For any path with more than 3 segments, the link 404'd because the URL was missing the leading directory components. The `file_link` helper now applies `rel_path` only to the display half and uses the full path in the URL. Regression test `dupes: deep-path URL keeps full path` covers a 6-segment monorepo path.
- **PR / MR comment status bar pluralizes `code issue(s)`, `clone group(s)`, and `health finding(s)` when the count is 1.** Previously the one-line summary read ungrammatically as `**1** code issues · **1** clone groups · **1** health findings`. Each noun now branches on `count == 1` so a single-finding PR reads `**1** code issue · **1** clone group · **1** health finding`. Same fix in both the GitHub Action and GitLab CI variants of `summary-combined.jq`. Regression tests in `action/tests/run.sh` and `ci/tests/run.sh` cover all three nouns.
- **Turborepo `_generators/config.ts` is detected as a generator config and credited.** The Turborepo plugin previously only looked for the canonical `turbo.json` location; projects that ship a Turborepo Custom Generator alongside other monorepo configs (in `<root>/turbo/generators/config.ts` or per-package `<package>/_generators/config.ts`) saw the generator file flagged as `unused-file`. The plugin now treats those locations as additional generator-config entry points and recurses through them. Thanks [@filipw01](https://github.com/filipw01) for the patch. ([#312](https://github.com/fallow-rs/fallow/pull/312))
- **Plugin-scoped hidden directories (`.client`, `.server`, ...) are traversed by every discovery consumer, not just dead-code analysis.** The first cut wired `discovery_hidden_dir_scopes` into the analyze pipeline, so dead-code saw plugin-contributed hidden folders correctly, but `list`, `dupes`, `health`, `flags`, `coverage`, and the rest of the discovery consumers re-walked the project with the default hidden-directory allowlist and skipped those folders. React-Router-Modules-style projects with `app/.client/` and `app/.server/` saw inconsistent file sets between commands. Every discovery consumer now reads the same plugin-scoped hidden-dir list, so coverage of `.client` / `.server` / equivalent plugin-contributed directories is uniform across the toolchain. Thanks [@jadengis](https://github.com/jadengis) for the patch. (Closes [#309](https://github.com/fallow-rs/fallow/issues/309))

### Changed

- **PR / MR comment Duplication section lists the actual clone locations instead of aggregate counts only.** The GitHub Action and GitLab CI top-level summary previously rendered a metric table (`Duplicated lines: 7`, `Clone instances: 2`, `Files with clones: 1`) and left readers to dig through the inline review comments to find the file paths and line ranges. The block now opens a top-5 locations table sorted by `line_count` descending (largest extractable wins first), with each row stacking the clone instances as `path:start-end` ranges joined by `<br>`. Aggregate counts move into the `<details>` summary header (`(N group(s) · M lines · X%)`) so the closed dropdown is still self-explanatory, and a footer states how many distinct files contain clones. When `GH_REPO` + `PR_HEAD_SHA` (Action) or `CI_PROJECT_URL` + `CI_COMMIT_SHA` (GitLab) are set, each cell links to the GitHub or GitLab blob URL with a line-range fragment so a click lands at the right span. Singular/plural grammar is rendered in the header and footer (1 group vs N groups, 1 file vs N files). Top-5 truncation matches the Complexity section's behaviour, with an "and N more groups" overflow line. The same shape is mirrored into the standalone `fallow dupes` summary so users hitting that surface no longer see a bare file list with no line ranges.

### Fixed

- **Jest plugin follows the `projects` field across all three of its real-world shapes (string globs, `package.json#jest` keys, and inline ProjectConfig objects).** A root `jest.config.js` that delegates discovery via `projects: ['<rootDir>/packages/*']` (or the file-shaped `projects: ['<rootDir>/packages/*/jest.config.{js,ts}']` variant) previously left fallow blind to every child config. Each child's `preset`, `transform`, `reporters`, `setupFiles`, `globalSetup`, `snapshotSerializers`, `testRunner`, `runner`, and other referenced packages went uncredited, so monorepo projects saw spurious `unused-dependency` / `unlisted-dependency` reports for everything only used inside per-package Jest configs. The plugin now expands each `projects` entry against the filesystem (with `<rootDir>` substitution and glob support), probes resolved directories for `jest.config.{ts,js,mjs,cjs,json}` first and falls back to `package.json` carrying a top-level `"jest"` object key, and recurses with cycle protection (canonicalised-path visited set, depth-bounded at 4) plus two complementary caps for pathological globs (raw glob iteration capped at 1024 matches inspected, accepted child configs capped at 64). Each child runs into a scratch result so its `testMatch` / `testRegex` / `replace_entry_patterns` stay scoped to that project; only `referenced_dependencies` and `setup_files` merge back into the parent, matching Jest's per-project semantics so a narrow `testMatch` in one package does not silently drop sibling packages' tests from entry-point analysis. Inline `ProjectConfig` objects (`projects: [{ preset: "ts-jest", runner: "jest-runner-eslint" }]`) are also followed: each object's package-typed fields (`preset`, `runner`, `testRunner`, `testEnvironment`, `resolver`, `snapshotSerializers`, `watchPlugins`, `reporters`) are credited as referenced dependencies and its setup files (`setupFiles`, `setupFilesAfterEnv`, `globalSetup`, `globalTeardown`) are credited as setup entry points, with the same builtin-runner / builtin-environment / relative-resolver filters the top-level extractor applies.

## [2.67.0] - 2026-05-07

### Added

- **`fallow audit` runs through `command: audit` in the GitHub Action and `FALLOW_COMMAND=audit` in the GitLab CI template.** Wrapping the severity-aware new-only PR gate previously required dropping to a manual `npx fallow audit ...` step alongside the action, because the action's `command:` validation rejected `audit` and there was no plumbing for `gate`, `coverage`, `max-crap`, or per-analysis baselines. Both wrappers now accept `audit` as a first-class command and forward `gate` (`new-only` / `all`), `coverage` + `coverage-root`, `max-crap`, and the three `dead-code-baseline` / `health-baseline` / `dupes-baseline` inputs to the CLI. The `outputs.issues` counter for audit returns the gate-aware introduced-finding count (sums `attribution.*_introduced` under `gate: new-only`, sums `summary.*` totals under `gate: all`) so downstream workflows comparing `outputs.issues > N` keep working without changes. PR comments render the verdict banner with per-category attribution tables (dead-code, complexity, duplication) plus a coverage-model footer that hints at `--coverage-root` when Istanbul match rate is low. Annotations and review comments fan out per-category through the existing comment and annotation jq scripts. Thanks [@hc-12](https://github.com/hc-12) for the report. (Closes [#302](https://github.com/fallow-rs/fallow/issues/302))
- **`fallow audit --coverage` / `--coverage-root` forward Istanbul coverage data to the embedded health analysis for accurate CRAP scoring.** Audit's CRAP threshold check previously saw the static estimate even when the project had real coverage data, so PRs touching well-tested code drew noisy `--max-crap` violations. The new `--coverage <path>` flag (or `FALLOW_COVERAGE` env var, or the MCP audit tool's `coverage` param) feeds the same `coverage-final.json` shape that `fallow health --coverage` accepts, and `--coverage-root` rebases CI-side absolute paths onto the local checkout when needed. The base-snapshot cache key folds in a content hash of the coverage file so cache entries invalidate when coverage changes. As a side fix, relative `--coverage` paths now resolve against `--root` rather than the process cwd, matching every other path-shaped CLI input (this also covers standalone `fallow health --coverage`). Empty-string MCP `Option<String>` params (`coverage: ""`, etc.) are dropped before reaching clap, so agents that emit empty-string-for-no-value no longer end up with `--coverage ""`. (Closes [#300](https://github.com/fallow-rs/fallow/issues/300))

### Fixed

- **Statically analyzable dynamic imports (`import('./foo')` with a literal specifier) flow through the unresolved, unlisted, member-usage, and external-style analyzers exactly like static imports do.** Until now, `import('@scope/pkg')` and `import('./foo')` produced graph edges and entry-point coverage, but the per-finding analyzers (unresolved imports, unlisted dependencies, dependency usage, unused-class-member lookup, external CSS package resolution) walked only `static_imports` and `requires`, so a literal dynamic import to a missing file or an unlisted package was silently elided from those reports. A shared `ResolvedModule::all_resolved_imports()` iterator now includes `dynamic_imports` whose specifier kind is `Literal`; pattern-shaped dynamic imports and graph-edge construction are unchanged. Suppression directives apply uniformly across all three import shapes. Thanks [@hussainarslan](https://github.com/hussainarslan) for the patch and [@filipw01](https://github.com/filipw01) for the report. (Closes [#304](https://github.com/fallow-rs/fallow/issues/304))
- **Cross-package consumers of a namespace re-exported through an object literal credit the original member.** When a barrel does `import * as foo from './bar'; export const API = { foo }` and a downstream package accesses `API.foo.bar`, `bar` was reported as `unused-export` because the binding-target metadata that ties `API.foo` to the namespace `foo` stayed local to the barrel file's extractor and never reached the consumer's resolution path. The visitor now collects per-module `NamespaceObjectAlias` entries (one per `(exported-object, suffix, namespace-local)` triple) at finalize time, the graph layer plumbs them through `ResolvedModule`, and a new Phase 2b pass walks each consumer's `member_accesses` for `<imported>.<suffix>` and credits the matching member on the namespace's source file. Star-barrel namespace targets (`./foo/index.ts` doing `export * from './bar'`) reuse the same synthetic-export helper that direct-namespace narrowing uses, so the credit lands on a stub that Phase 4 chain resolution then propagates to the real defining file. Same-file aliases (issue #269) keep working unchanged; nested suffixes such as `motionNet.adEngine` are handled via dotted-path matching. Targeted to the alias pattern: zero delta on all eight benchmark fixtures (preact, zod, svelte, vue-core, fastify, query, vite, next.js). Bumps the extract `CACHE_VERSION` so warm caches do not retain pre-fix data. Thanks [@filipw01](https://github.com/filipw01) for the report and the clean reproduction repo. (Closes [#303](https://github.com/fallow-rs/fallow/issues/303))
- **Structured fallow errors no longer slip past the GitHub Action and GitLab CI wrappers as a green run.** When fallow fails on a config or validation error it emits `{"error":true,"message":"...","exit_code":N}` on stdout. Both wrappers' issue-counter jq filters used `// 0` defaults, so a null-on-error-shape silently became `issues=0` and the workflow went green even though fallow exited non-zero. Both wrappers now check `jq -e '.error == true' fallow-results.json` after capture and before counting, surface the structured error message, and exit with the CLI's reported `exit_code`. The same path intercepts the `audit` + generic `baseline` / `save-baseline` misuse pre-spawn with a message pointing at the three `dead-code-baseline` / `health-baseline` / `dupes-baseline` inputs (audit's three sub-analyses use incompatible baseline formats).
- **GitHub Action and GitLab CI threshold gates branch on the audit verdict, not the raw introduced-finding count.** `fallow audit` returns a severity-aware verdict (`pass` / `warn` / `fail`) where `warn` means "warn-tier issues only, do not fail CI". The wrappers' threshold step previously gated on `attribution.*_introduced > 0`, so a project with `unused-exports: warn` and the action default `fail-on-issues: true` saw CI fail on every PR that touched a warn-tier finding even though the verdict said "do not fail". Both wrappers now extract `.verdict` and `.attribution.gate` from audit JSON, expose them as `outputs.verdict` / `outputs.gate` (Action) / equivalent script vars (GitLab), and the Check threshold step branches on `command=audit`: gates on `VERDICT==fail` for audit, falls through to the existing `ISSUES>0` logic for every other command. Fail-tier still fails as expected. Refs [#302](https://github.com/fallow-rs/fallow/issues/302).
- **`--coverage-root` rejects relative paths with an actionable error instead of silently no-op'ing.** `--coverage-root` is a strip-prefix argument applied to absolute paths inside Istanbul coverage data (CI runners emit paths like `/ci/workspace/src/index.ts`, and the user passes `/ci/workspace` as the prefix to rebase). Relative values such as `src` cannot match those paths under any normalisation, but the previous behaviour was to absolutise them against `--root` and proceed, producing an unmatched prefix and zero coverage matches with no signal to the user. The validator at `health::scoring::validate_coverage_root_absolute` now exits 2 with `--coverage-root expects an absolute path prefix from the coverage data, got 'src'. Use the checkout prefix from the machine that generated coverage, for example '/home/runner/work/myapp'.` Runs at every entry point that accepts the flag (`audit::run_audit`, `health::execute_health`, combined-mode bare run, the NAPI `programmatic::compute_complexity` binding, plus a defence-in-depth check at the Istanbul load site). The companion `--coverage` file flag keeps its root-relative resolution since it points at a file fallow opens.
## [2.66.2] - 2026-05-07

### Fixed

- **`fallow audit` no longer fails when invoked from a `pre-commit` / `pre-push` hook.** When git runs hooks it exports ambient repo-state environment variables into the hook subprocess (`GIT_INDEX_FILE=.git/index` as a relative path, plus `GIT_PREFIX` and friends). Audit's `git worktree add` was inheriting those, and the relative `GIT_INDEX_FILE` failed to resolve from the temporary worktree directory, so worktree creation aborted with `could not create a temporary worktree for base ref 'main'` (workaround was `env -u GIT_INDEX_FILE fallow audit`). Every production `git` invocation in fallow now strips the documented set of ambient repo-state vars (`GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_COMMON_DIR`, `GIT_PREFIX`) via a shared helper, so lint-staged, husky, lefthook, and the `pre-commit` framework all work transparently. The same robustness applies to churn / hotspot, regression baselines, coverage uploads, init's branch detection, and vital signs. The worktree-creation error also surfaces a hint when an ambient repo-state var is detected so users on older fallow versions can self-rescue. Thanks [@videvian](https://github.com/videvian) for the report. (Closes [#301](https://github.com/fallow-rs/fallow/issues/301))

## [2.66.1] - 2026-05-06

### Changed

- **User-facing CLI output uses ASCII punctuation in place of em-dashes.** Rule descriptions in `fallow explain`, the top-level `fallow --help` about line, `fallow list` boundary output, the JSON `actions` note for refactor-together clone groups, the markdown health metric legend, hotspot stderr notes, the LSP test-only-dep diagnostic, the MCP `list_boundaries` tool description, and a handful of lower-traffic strings are now em-dash-free. Internal asserts, code comments, and clippy `#[expect]` reasons are out of scope.

### Fixed

- **Astro `<script src="...">` and inline `<script>` ESM imports are followed during static analysis.** Astro pages route per-component client code through two template-side shapes the extractor previously did not see: external `<script src="../scripts/foo.ts"></script>` references and inline `<script>import '../scripts/bar';</script>` ESM bodies. Targets of either form are bundled into the build output but were surfacing as `unused-file`. The Astro parser now scans the post-frontmatter template body for external script and link references via the shared HTML asset-ref collector, and parses inline `<script>` bodies as TypeScript to extract their ES module imports. Astro only processes a `<script>` tag when it has zero attributes or only `src=` (any other attribute, e.g. `is:inline` or `type="module"` with extras, renders as authored without bundling), so attributed inline scripts are intentionally excluded from the import graph to match Astro's own behavior. HTML comments are stripped before scanning, and inline bodies are skipped when a `src=` attribute is present so external references are never double-counted. Thanks [@zaphir](https://github.com/zaphir) for the report. (Closes [#295](https://github.com/fallow-rs/fallow/issues/295))
- **`node:module` `register()` loader specifiers credit their target packages.** `register('@swc-node/register/esm', pathToFileURL('./'))` from `node:module` (or the bare `module` builtin) loads a loader module by string specifier rather than via a static or dynamic import, so registered packages like `@swc-node/register` and `tsx` were surfacing as `unused-dependency` even though Node loads them at startup. The visitor now records the first argument of `register(...)` calls as a require / dynamic-import specifier when the callee resolves to the `register` export of `node:module` / `module`. Named imports (`import { register }`), aliased named imports (`import { register as reg }`), and namespace imports (`import * as Module from 'node:module'; Module.register(...)`) all engage; same-named functions from any other source are skipped. Template-literal and dotted-call first-argument forms are recognized too. Thanks [@ChrisJr404](https://github.com/ChrisJr404) for the patch and [@pdeveltere](https://github.com/pdeveltere) for the report. (Closes [#293](https://github.com/fallow-rs/fallow/issues/293))
- **`fallow audit` base-side worktrees see the same `node_modules` as HEAD.** Audit base worktrees lacked `node_modules`, breaking tsconfig `extends` chains that point into installed packages and disabling path-alias resolution on the BASE side. The result was spurious `Broken tsconfig chain` warnings and false-positive attributions on React Native repos whose tsconfig extends `./node_modules/@react-native/typescript-config/tsconfig.json`. Fresh, reusable, and ready persistent base worktrees now symlink `repo_root/node_modules` so the BASE-side analysis sees the same dependency context as HEAD. The audit base snapshot cache version is bumped so prior snapshots captured under the broken behavior are invalidated. Thanks [@iarmankhan](https://github.com/iarmankhan) for the report. (Closes [#292](https://github.com/fallow-rs/fallow/issues/292))
- **Local tsconfig `paths`, `baseUrl`, and `rootDirs` stay usable when an `extends` chain is broken.** When a tsconfig's `extends` target was missing or unparsable, the resolver previously fell back to a no-aliases mode for the entire config, so valid local imports against `paths` defined in the same tsconfig surfaced as `unresolved-import`. The graph resolver now keeps the local tsconfig's alias contract alive and only drops the broken-extends parent's contributions, so React Native and Expo projects whose framework base tsconfig is untracked or out of sync continue to resolve their own aliases. Audit base snapshots also compare against the same merged config contract as the current run, with new test coverage for React Native / Expo aliases, config inheritance, project references, cache hits, and import edge shapes. Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza) for the patch. ([#299](https://github.com/fallow-rs/fallow/pull/299))
- **Combined-mode `--format json` outside a git repository is now exactly one JSON document.** When the project root sat outside any git checkout, the hotspot pipeline emitted a structured `{"error": true, "message": "hotspot analysis requires a git repository", ...}` blob to stdout and then combined mode appended its normal report on top, so `fallow --format json | jq .` and any agent / CI parser failed with `trailing characters at line 6 column 1`. Missing git history is now treated as unavailable hotspot data: stdout stays a single document with empty hotspots, a non-fatal `note: hotspot analysis skipped: no git repository found at project root` goes to stderr (suppressed by `--quiet`). Standalone `fallow health --hotspots --format json` outside a git repo now exits 0 with empty hotspot fields instead of exiting 2 with a JSON error; CI scripts that depended on the old exit-2 signal need to inspect the (now omitted) `hotspot_summary` field instead. Thanks [@ChrisJr404](https://github.com/ChrisJr404) for the patch. (Closes [#294](https://github.com/fallow-rs/fallow/issues/294))

## [2.66.0] - 2026-05-06

### Added

- **Prisma `generator { provider = "..." }` blocks credit custom-generator npm packages.** The prisma plugin now scans `prisma/schema.prisma`, root-level `schema.prisma`, and the multi-file `prisma/schema/*.prisma` layout for `generator <name> { provider = "<pkg>" }` blocks and credits each non-built-in provider as a referenced dependency, so packages like `prisma-json-types-generator` and `prisma-erd-generator` no longer surface as `unused-dependency`. `datasource` providers (`postgresql`, `mysql`, etc.), the shell-command form (`provider = "node ./gen.js"`), and the relative-path form (`provider = "./local-generator"`) are intentionally skipped; line and block comments are stripped before scanning so commented-out generators do not produce phantom credits. (Closes [#288](https://github.com/fallow-rs/fallow/issues/288))
- **Custom Prisma schema paths declared in `prisma.config.ts` are honored.** The plugin now reads the static `schema` field from `prisma.config.{ts,mts,cts,js,mjs,cjs}` (or the new `.config/prisma.{ts,...}` Prisma 6 alternate location), resolves the path against the project root, marks the configured schema file or folder as always-used, and recursively scans it for generator providers. Layouts where the schema sits at e.g. `db/schema/` instead of the canonical `prisma/schema/` are now covered without needing `ignoreDependencies`. Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza) for the patch. ([#291](https://github.com/fallow-rs/fallow/pull/291))
- **VS Code extension: client-side diagnostic mute and dynamic issue types.** Diagnostics can be muted from the editor without round-tripping through `.fallowrc.jsonc`, and the issue-type list shown in the LSP filter is now populated dynamically from the language server instead of a hardcoded enum, so newly-added issue kinds appear without an extension release. Thanks [@FunctionDJ](https://github.com/FunctionDJ) for the report. (Closes [#287](https://github.com/fallow-rs/fallow/discussions/287))

### Changed

- **Plugin discovery skips the filesystem walk for source-extension config patterns.** Patterns like `webpack.config.{ts,js,mjs,cjs}` describe source files that are already in the file index, so re-stat'ing them once per plugin during `discover_config_files` was redundant. Root-anchored patterns whose extensions are all in `SOURCE_EXTENSIONS` and that have no path separator or leading dot are now wrapped with a `**/` prefix and matched against the in-memory file set instead. Production mode (which excludes `*.config.*` from the source walker) preserves the filesystem fallback for those patterns to keep correctness. Measured on a 21,033-file Next.js monorepo: bare `fallow` 9.7s to 3.3s (-66%), `fallow audit --gate all` 11.7s to 5.4s (-54%), plugins stage alone 7.4s to 1.4s (-82%). Smaller projects show no regression.
- **`fallow audit` caches base-branch snapshots and parallelizes HEAD analyses against base-snapshot computation.** Repeat audit runs on the same base reuse cached dead-code + dupes snapshots, and the worktree analysis runs concurrently with the base snapshot rebuild via rayon, so `audit` finishes closer to the slower of the two halves rather than their sum.
- **`fallow` (combined mode) runs check and dupes concurrently via `rayon::join`.** The two analysis trees are independent; running them on separate worker pools shaves wall-clock without changing per-stage CPU budget.

### Fixed

- **`fallow dupes` honors `duplicates.{minLines, minTokens, threshold, mode, skipLocal}` from `.fallowrc.jsonc` when the matching CLI flag is omitted.** Standalone `fallow dupes` previously stomped these config values with the clap defaults (`minLines=5`, `minTokens=50`, etc.) because the CLI args were typed as `usize`/`f64` with `default_value` attributes, so "flag omitted" was indistinguishable from "user passed the default". The CLI scalars are now `Option<T>`, an absent flag falls through to the toml value, and `skipLocal` adopts the OR-merge already used for `crossLanguage` / `ignoreImports`. The failure threshold for the gate now sources from the merged config too, so `--threshold` omitted no longer disables a config-declared gate. `audit`, `combined`, and the programmatic API are unaffected (they merge config themselves and pass explicit `Some(...)` values). Thanks [@ryota-murakami](https://github.com/ryota-murakami) for the patch. ([#290](https://github.com/fallow-rs/fallow/pull/290))
- **`fallow dupes --performance` actually emits the duplication performance panel.** The flag was parsed by clap but never plumbed into `DupesOptions`, so it was silently a no-op even though combined-mode `fallow --performance` already printed a duplication stage in its panel. The standalone command now emits a `Duplication Performance` panel on stderr (human / compact / markdown only; structured formats stay clean).

## [2.65.0] - 2026-05-05

### Added

- **Lit and Web Components classes registered through runtime side effects are credited as used.** Two patterns are now recognized end-to-end: Lit's `@customElement('tag')` decorator on a class (named-import form, aliased imports, namespace-call form like `@decorators.customElement('tag')`, and anonymous `export default @customElement('x') class extends LitElement {}`), and `customElements.define('tag', ClassRef)` calls at any depth. The decorator binding is verified against `lit/decorators.js` / `lit/decorators/custom-element.js` before crediting, so a same-named decorator from any other module is intentionally ignored. A new `lit` plugin contributes heritage-scoped allowlists for Lit lifecycle methods (`render`, `updated`, etc.) on `LitElement` / `ReactiveElement` subclasses; native Custom Elements lifecycle members (`connectedCallback`, `observedAttributes`, etc.) are built-in heritage-scoped allowlists for `HTMLElement` subclasses so they work without a Lit dependency. Non-lifecycle methods on Web Component classes are still reported.
- **`schema.json` is bundled inside the published `fallow` npm package.** Consumers can now write `"$schema": "./node_modules/fallow/schema.json"` for version-aligned config validation with no network round-trip to `raw.githubusercontent.com`. The release workflow copies `schema.json` into the wrapper before publishing, and CI asserts the file is present in every published tarball so the package never silently loses it. Thanks [@ChrisJr404](https://github.com/ChrisJr404) for the patch and [@OmerGronich](https://github.com/OmerGronich) for the report. (Closes [#275](https://github.com/fallow-rs/fallow/issues/275))

### Fixed

- **Angular signal-based query factories and plural decorator queries are now traced for `unused-class-members`.** `viewChild()`, `viewChildren()`, `contentChild()`, `contentChildren()` properties (`readonly vc = viewChild<ChildComponent>('vc')`) and `@ViewChildren` / `@ContentChildren` properties typed as `QueryList<T>` previously produced false `unused-class-member` findings on the child component's methods because the call-graph could not see through `this.vc()?.refresh()` (signal queries return a callable signal) or `this.dvcs?.forEach(c => c.refresh())` (the iteration parameter had no resolved type). The visitor now extracts the query's element type (explicit `<T>` type argument or first identifier argument for the signal factories, peeled `QueryList<T>` annotation for the plural decorators) and registers it against a synthetic call-form binding (`this.vc()` to `T`) for the singular signal queries, and against an iterable map (`this.vcs()` / `this.dvcs` to `T`) for the plural cases. `forEach` arrow callbacks on a registered iterable bind their first parameter to the element type so the existing bound-member-access pipeline credits the child's methods. The pre-existing `@ViewChild` and `@ContentChild` paths continue to work unchanged. Thanks [@ChrisJr404](https://github.com/ChrisJr404) for the patch and [@OmerGronich](https://github.com/OmerGronich) for the eight-pattern reproducer. (Closes [#274](https://github.com/fallow-rs/fallow/issues/274))
- **`vite.config.{ts,js,mts,mjs}` default export is no longer flagged under `--include-entry-exports`.** The vite plugin now contributes `used_exports` for `vite.config.*` (default), mirroring the existing vitest treatment. With `--include-entry-exports` the strict reachability check previously surfaced the default export even though Vite's CLI consumes it. Thanks [@ChrisJr404](https://github.com/ChrisJr404) for the patch and [@filipw01](https://github.com/filipw01) for the report. (Fixes [#282](https://github.com/fallow-rs/fallow/issues/282))
- **`prisma.config.{ts,mts,cts,js,mjs,cjs}` recognized as an entry point.** The prisma plugin now treats the new Prisma 6 config file as an entry, so the file (and any imports it reaches) stays alive in the graph and does not surface as `unused-file`. Thanks [@ChrisJr404](https://github.com/ChrisJr404) for the patch and [@FunctionDJ](https://github.com/FunctionDJ) for the report. (Closes [#281](https://github.com/fallow-rs/fallow/issues/281))
- **`fallow migrate` accepts JSONC trailing commas.** Real-world JSONC files (`tsconfig.json`, `.vscode/settings.json`, and similar) routinely trail commas before `}` / `]`. `load_json_or_jsonc` ran the input through comment-stripping and then handed the result to `serde_json`, which rejects trailing commas. A final byte-level pass now strips them only when the comment-stripped parse fails, leaves commas inside string literals untouched, and still rejects genuinely malformed input like `{,}` (the `comma_follows_json_value` predicate keeps malformed leading-commas reporting as parse errors). Thanks [@ChrisJr404](https://github.com/ChrisJr404) for the patch and [@madflow](https://github.com/madflow) for the report. (Closes [#276](https://github.com/fallow-rs/fallow/issues/276))
- **Vue `generic` and Svelte `generics` script-tag attributes scan for type references.** A type-only import whose only consumer was a generic constraint on the `<script>` tag (`<script lang="ts" generic="T extends Test<boolean>">` for Vue, `<script lang="ts" generics="T extends Item">` for Svelte) was falsely flagged as `unused_types` because the constraint lives on the tag, not in the script body. The SFC parser now appends an augmented-source probe that re-introduces the constraint to the parse so the imported type's binding shows up as referenced and `oxc_semantic` no longer classifies it as unused.

## [2.64.0] - 2026-05-04

### Added

- **Webpack `resolve.alias` and entry `context` are parsed from `webpack.config.{js,ts,cjs,mjs}`.** Aliases written as `'@components': path.resolve(__dirname, 'src/components')` (or `path.join(__dirname, ...)`, or plain string values) now feed the resolver, matching what Vite, Nuxt, and SvelteKit already do. Aliased imports (`import { Button } from '@components/Button'`) no longer surface as `unresolved-import`, and files reachable only through an alias no longer cascade as `unused-file` / `unused-export`. The Webpack 5 entry-descriptor shape (`entry: { app: { import: './src/app.ts' } }`), object-array entries (`entry: { app: ['./polyfill.ts', './app.ts'] }`), and the optional top-level `context: path.resolve(__dirname, 'src')` are also recognized; entries are normalized against the context path so descriptor + context configurations resolve end-to-end. Dynamic and function-valued entries remain out of scope; static evaluation of those would require a much larger change. Thanks [@michaljuris](https://github.com/michaljuris) for the detailed report including the workaround analysis. (Closes [#273](https://github.com/fallow-rs/fallow/issues/273))
- **`tap` and `tsd` test runners get built-in plugin support.** The `tap` plugin activates from a `tap` dependency and treats node-tap's default test discovery (`test/`, `tests/`, `__tests__/`, `*.test.*` / `*.spec.*`, top-level `test.*` / `tests.*`) plus `.taprc` configs as reachable. The `tsd` plugin activates from a `tsd` dependency and treats `.test-d.ts(x)` declaration tests plus `package.json#tsd.directory` as reachable. Test-only code in either runner no longer surfaces as `unused-file`.

### Fixed

- **Bundler entry patterns with a leading `./` now resolve.** Webpack/Rollup/Rspack/Rsbuild/Rolldown configs commonly write `entry: './src/app.ts'` or `input: './src/index.js'`. The entry-pattern matcher compiles globs with `literal_separator(true)`, so a `./src/app.ts` pattern would never match the project-relative path `src/app.ts` in the file index, and the entry was silently dropped. `extend_entry_patterns` and `push_entry_pattern` now strip the prefix at the push site, so descriptor entries without an accompanying `context:` and plain string entries with `./` both resolve end-to-end.
- **`import * as ns from './x'` namespace member access is credited through re-exporting barrels.** A namespace import that flows into an object literal (`const API = { motionNet: { adEngine } }`) and is then read via a chained property access (`API.motionNet.adEngine.getMetaAssetsTeam`) now correctly credits `getMetaAssetsTeam` as used, even when the namespace target is itself a `export * from './queries'` barrel. Previously the leaf access stopped tracking at the object literal and the `getMetaAssetsTeam` export surfaced as unused. Thanks [@filipw01](https://github.com/filipw01) for the report with a complete reproduction. (Closes [#269](https://github.com/fallow-rs/fallow/issues/269))
- **Public-package class members are no longer flagged as unused.** When a workspace package is listed in `publicPackages` (because it's published as a library), classes re-exported from the package's entry file are part of the public API. Member methods that aren't called inside the workspace previously surfaced as `unused-class-member` even when their containing class was clearly part of the package's exported surface. The same logic that already suppressed enum members on public packages now extends to class members. Non-public packages still report unused members so consumers of the workspace's own internal classes still benefit from member-level dead-code detection. Thanks [@ghost23](https://github.com/ghost23) for the report. (Closes [#267](https://github.com/fallow-rs/fallow/issues/267))
- **Playwright Page Object Model methods consumed through nested fixture types are credited.** Fixtures with nested object-literal types (`{ pages: { adminPage: AdminPage } }`) and fixtures that reference a named type alias for the inner shape (`type PageFixtures = { adminPage: AdminPage }; type MyFixtures = { pages: PageFixtures }`) now credit POM methods accessed through the dotted path (`pages.adminPage.assertGreeting()`). The visitor walks both the type-side and the destructure-side via a dotted path, so multi-level `{ pages: { adminPage } }` destructures and `await pages.adminPage.method()` chains both flow into use credits. Thanks [@vethman](https://github.com/vethman) for the report. (Closes [#268](https://github.com/fallow-rs/fallow/issues/268))
- **`.gts` (Glimmer TypeScript) imports honor `tsconfig.json#paths` aliases.** Imports inside `.gts` files now go through the same TypeScript path-alias resolver the rest of the codebase uses, so Ember + Glimmer projects with `paths: { "@app/*": ["src/*"] }` no longer surface every aliased import as `unresolved-import`. Thanks [@square-brackets](https://github.com/square-brackets) for the report. (Closes [#270](https://github.com/fallow-rs/fallow/issues/270))
- **`vitest.config.*` default exports and Storybook story conventions stop appearing as unused under `--include-entry-exports`.** With `--include-entry-exports`, the strict reachability check previously flagged `vitest.config.ts`'s default export and `*.stories.*` / `.storybook/**` exports even though they're framework-consumed. The Vitest plugin now contributes `used_exports` for `vitest.config.*` and `vitest.workspace.*` (default), and the Storybook plugin contributes a `*` wildcard for `**/*.stories.*` and `.storybook/**`. The wildcard required restoring symmetry in `is_export_ignored` so plugin-supplied `used_exports` honor `*` the same way user-config `ignoreExports` already did. Thanks [@filipw01](https://github.com/filipw01) for the report. (Fixes [#271](https://github.com/fallow-rs/fallow/issues/271))

## [2.63.0] - 2026-05-04

### Added

- **Vitest `/__mocks__` virtual specifiers are no longer flagged as unlisted dependencies.** Vitest's manual-mock convention places mock factories at `<package>/__mocks__/<module>.ts` and some test setups also import directly from `@<scope>/__mocks__` paths via `package.json#imports` aliases or workspace virtual paths. `@aws-sdk/__mocks__`, `@sentry/__mocks__`, `@supabase/__mocks__`, etc. do not exist on npm and previously triggered an `unlisted-dependency` finding with an "install this package" auto-fix that pointed at a package that does not exist. The Vitest plugin now contributes a `/__mocks__` package-name suffix to the new `Plugin::virtual_package_suffixes()` trait method; specifiers ending in that suffix are suppressed from `find_unlisted_dependencies`. The suffix list merges across workspace plugin runs into the root `AggregatedPluginResult`, so monorepos that put Vitest only in a workspace's `package.json` (not the root) get the same suppression. Thanks [@fmguerreiro](https://github.com/fmguerreiro) for the report and the fix. ([#265](https://github.com/fallow-rs/fallow/pull/265))

### Changed

- **`health_score` is now scale-invariant.** The penalty formula previously used absolute counts (`unused_dep_count`), unweighted averages (`avg_cyclomatic`), and order-statistics (`p90_cyclomatic`) that are mathematically incapable of firing at large-monorepo scale: a 50k-LOC monorepo with 200 unused devDependencies and 1500 functions over 60 LOC scored in the B band because the per-dimension caps were saturated and the averages were diluted by clean code in the long tail. The reworked formula switches to scale-invariant aggregators: `critical_complexity_pct` (functions over a hard CC threshold), `maintainability_low_pct` (files below the MI threshold), `unused_deps_per_k_files`, `circular_deps_per_k_files`, `functions_over_60_loc_per_k`, `coupling_high_pct`, and `hotspot_top_pct_count` (top-percentile hotspots normalized against `total_files`). Caps on `unused_deps` and `circular_deps` raised from 10 to 25, allowing those dimensions to actually contribute at monorepo scale. New `formula_version: 2` field on `HealthScore` lets consumers detect the formula change. Older snapshots that lack the scale-invariant fields fall back to the previous aggregators so cached / archived data still scores. Thanks [@OmerGronich](https://github.com/OmerGronich) for the detailed report including the per-dimension cap analysis. (Closes [#260](https://github.com/fallow-rs/fallow/issues/260))

### Fixed

- **CSS `@import 'pkg/subpath.css'` resolves through `package.json#exports` with the `style` condition.** Bare CSS imports whose target is exposed only through an `exports` map under the `"style"` condition (the canonical pattern for design-system packages like shadcn, daisyui, and Tailwind v4 plugins) previously surfaced as `unresolved_imports` even when the file existed and bundlers resolved it correctly. The CSS / SCSS resolver now consults the package's `exports` map for the requested subpath before falling back to the `node_modules/<pkg>/<file>` direct path, picking up `{ "./tailwind.css": { "style": "./dist/tailwind.css" } }` shapes that target arbitrary subdirectories. Thanks [@VidhyaKumar](https://github.com/VidhyaKumar) for the report with a complete shadcn 4.6.0 reproduction. (Closes [#261](https://github.com/fallow-rs/fallow/issues/261))
- **CI YAML scanner stops emitting `WARN invalid entry pattern` for shell and regex fragments.** GitHub Actions expressions (`${{ env.URL }}/api/health`), `jq -r '.[]'` array iterators, and Perl regex shards (`grep -oP '(?<=Module )\./[^ ]+'`) split on whitespace into tokens like `}}/api/health`, `'.[]'`, and `)\./[^` that reached `globset::GlobBuilder::new(...).build()` and produced 10+ noise warnings on a typical CI repo. A new `could_be_file_path` negative-only guard rejects tokens whose syntax precludes a Unix path (unbalanced `${{`/`}}`, backslashes, malformed `[...]`) before they reach globset compilation. The guard runs on `cmd.config_args` extracted from `--config`/`-c` flags and on `cmd.file_args` via the existing `looks_like_file_path` path. Next.js dynamic-route segments (`app/[id]/page.tsx`, `pages/[...slug].ts`) remain valid. Thanks [@fmguerreiro](https://github.com/fmguerreiro) for the report and the fix. ([#262](https://github.com/fallow-rs/fallow/pull/262))
- **Next.js `dynamic(() => import('./X').then(m => m.X))` lazy-loaded re-exports no longer flagged as `duplicate-export`.** The Next.js code-splitting idiom where `Foo-lazy.tsx` exports `Foo = dynamic(() => import('./Foo').then(m => m.Foo), { ssr: false })` is semantically a re-export of `Foo`, equivalent to `export { Foo } from './Foo'` (which fallow already does not flag). `find_duplicate_exports` now extends `re_export_sources` with dynamic-import edges that act as re-exports: a dynamic import targeting an internal module counts as a re-export only when the wrapper module exports the same shape (`Named("X")` requires the wrapper to export `X`; `Default` requires the wrapper to have a default export). The wrapper-must-export check guards against false negatives where a module dynamically imports something but does not actually re-export it. Thanks [@fmguerreiro](https://github.com/fmguerreiro) for the report and the fix. ([#263](https://github.com/fallow-rs/fallow/pull/263))
- **ESLint flat-config plugin imports trace through workspace-internal config packages.** Turborepo / Nx monorepos that centralize ESLint config in a workspace package (`packages/eslint-config` exporting from `index.js`) were producing false `unused-devdep` flags for plugins that the shared config imports transitively. Two cooperating fixes: the ESLint plugin's `read_package_entry_for_specifier` now walks up `start.ancestors()` checking `node_modules/<pkg>/package.json` (bounded by `MAX_NODE_MODULES_WALK_DEPTH = 8`), so packages hoisted to the monorepo root are found from a workspace root, and resolves `@scope/pkg/subpath` imports via the package's `exports` map with `.js`/`.mjs`/`.cjs` extension fallback. `must_parse_workspace_config_when_root_active` now includes `"eslint"` so when the root-level ESLint plugin is active, every workspace's `eslint.config.*` still gets parsed instead of being silently skipped by `run_workspace_fast`. Thanks [@fmguerreiro](https://github.com/fmguerreiro) for the report and the fix. ([#266](https://github.com/fallow-rs/fallow/pull/266))

## [2.62.0] - 2026-05-02

### Added

- **`usedClassMembers` entries accept glob patterns.** Member strings containing `*` or `?` now compile as glob matchers, so a single rule can cover the entire family of methods a framework dispatches reflectively. Use `"*"` to match every member on a heritage-matching class, `"enter*"` / `"*Handler"` for prefix or suffix matching, or `"on*Event"` for combined prefix+suffix. The shape (`{ extends?, implements?, members }`) is unchanged; exact-string entries still work as before. Glob patterns matching zero members across the codebase emit a `WARN` so dead allowlist entries surface. Designed for parser-generator listeners (ANTLR), code-generated bridges (`protoc-ts`, `openapi-typescript`, `graphql-codegen`), and abstract framework bases that dispatch on a member-name prefix instead of an enumerated list. Thanks [@OmerGronich](https://github.com/OmerGronich) for the report. (Closes [#254](https://github.com/fallow-rs/fallow/issues/254))
- **`overrides[].rules.circular-dependency: "off"` suppresses cycles whose files all match the override glob.** Previously `circular-dependency` was the only rule that ignored per-folder `overrides[]` entries; you had to disable cycle detection globally or add `// fallow-ignore-file circular-dependency` to every file in every cycle. Now a cycle is suppressed when **every** file in the cycle resolves to `Severity::Off` for `circular-dependency` via `overrides[]`. Cycles that touch even one non-overridden file remain reported, preserving real-positive detection on shared modules. Thanks [@OmerGronich](https://github.com/OmerGronich) for the report. (Closes [#255](https://github.com/fallow-rs/fallow/issues/255))
- **First-class blast-radius and importance sections on `fallow coverage analyze`.** New `--blast-radius` and `--importance` flags surface runtime-weighted blast-radius and importance findings in the human output, alongside the existing hot-path / cold-path sections.
- **Runtime coverage `--top` flag.** Limits the runtime findings + hot-path display to the top N entries, matching `fallow dead-code`'s top-N flag for parity.

### Changed

- **`analyze` stage runs detectors in parallel.** The pipeline's analyze stage now schedules its ten independent detectors (`unused_files`, `unused_exports`, `unused_members`, dependency detectors, `unresolved_imports`, `duplicate_exports`, `boundary_violations`, `circular_dependencies`, etc.) across rayon worker threads instead of serially on a single core. On a synthetic 24,320-file monorepo with realistic barrel files and cross-workspace imports the analyze stage drops from `~6.75s` to under half a second on a 14-core machine. Real-world fixtures (`next.js`, `preact`) show a `~2x` speedup on the analyze stage with byte-identical results. Find-unused-exports and find-unused-members also parallelise their inner module loops via rayon. Thanks [@OmerGronich](https://github.com/OmerGronich) for the report. (Closes [#259](https://github.com/fallow-rs/fallow/issues/259))
- **Git churn cache is incremental.** `.fallow/churn.bin` now stores per-commit events keyed by `last_indexed_sha`. When `HEAD` advances from the cached SHA, fallow runs `git log <cached>..HEAD --numstat` and merges the delta into the cached state instead of re-shelling out for the entire churn window. CI runs that fallow on every push and pre-commit hooks now hit the cache for the bulk of the work, paying only the marginal-commit cost. The cold path is unchanged. The cache automatically invalidates when the `--since` window changes or when `cache.last_indexed_sha` is no longer reachable from the current HEAD (force-push, branch switch). Thanks [@OmerGronich](https://github.com/OmerGronich) for the report. (Closes [#258](https://github.com/fallow-rs/fallow/issues/258))
- **`--performance` table includes the duplication stage.** The `Pipeline Performance` table on `fallow --performance` (combined mode) now prints a `duplication: <ms>` row alongside the other stages instead of leaving the cost as an easy-to-miss parenthetical in the body output. The dead-code-only and dupes-only timing breakdowns are unchanged. Thanks [@OmerGronich](https://github.com/OmerGronich) for the report. (Closes [#257](https://github.com/fallow-rs/fallow/issues/257))

### Fixed

- **`circular-dependency` line-level inline directives now actually suppress.** `// fallow-ignore-next-line circular-dependency` on the offending import line previously landed in `stale_suppressions` and the cycle still appeared in the output, even though `fallow dead-code --format json` recommended exactly that comment in `actions[]`. The directive now decrements the cycle count as expected, and IDE/CLI consumers of the recommended `actions[]` produce working patches instead of silent no-ops. Singular and plural slug aliases (`circular-dependency` vs `circular-dependencies`) are now interchangeable across inline directives, `rules`, and `overrides[].rules`, so the wrong-form / right-form mismatch between surfaces is resolved. Thanks [@pippenz](https://github.com/pippenz) for the report. (Closes [#256](https://github.com/fallow-rs/fallow/issues/256))
- **Bare `() => import('./X')` route callbacks credit the default export.** Object-literal properties named `component`, `loadChildren`, or `loadComponent` whose value is `() => import('./X')` (or a function-expression equivalent) now credit the target module's default export as used, even when no `.then(m => m.default)` is spelled. The control case (`.then(m => m.default)`) was already covered. Fixes the `unused-export` false positive on the standard Angular Router (`loadChildren: () => import('./feature.routes')`) and Vue Router (`component: () => import('./View.vue')`) lazy-loading shapes. The property-name list is an exact whitelist; other property names with the same shape (e.g. `loader`, `Component`) are not credited. Thanks [@OmerGronich](https://github.com/OmerGronich) for the report. (Closes [#253](https://github.com/fallow-rs/fallow/issues/253))
- **CSS `@import 'pkg/subpath.css'` resolves through `node_modules` for both relative and bare specifiers.** Tailwind v4 `@import 'tailwindcss/theme.css'` and `@import 'tailwindcss/utilities.css'` patterns no longer surface as unresolved imports or as unused-dependency on `tailwindcss`. The CSS extractor preserves the package subpath, the resolver now probes the npm package's subpath via the standard `node_modules` walk-up, and the dependency credit flows through the resolved hit.

## [2.61.0] - 2026-05-01

### Added

- **GraphQL document `#import` edges follow into the module graph.** `.graphql` and `.gql` files are now discovered as source files, and lines matching `#import "./fragment.graphql"` (or `# import '../shared/fragment.gql'`) emit `SideEffect` imports so fragment and schema documents reachable only through GraphQL import comments stay connected. Only relative string specifiers (`./`, `../`) are extracted; package-style imports are left to the resolver. Extensionless relative imports probe `.graphql` and `.gql` automatically. Thanks [@lsbyerley](https://github.com/lsbyerley) for the report. (Closes [#250](https://github.com/fallow-rs/fallow/issues/250))
- **Vitest `vi.mock()` credits the `__mocks__/` sibling.** `vi.mock('./services/api')` now synthesizes a dynamic namespace import of `./services/__mocks__/api`, crediting the auto-mock sibling file (and all of its exports) as used so vitest's `__mocks__/<file>` convention does not surface as `unused-file`. Handles string-literal sources, expressionless template literals, and the `vi.mock(import('./api'))` argument form. Path-alias prefixes (`@/src/...`) are preserved so the importer's tsconfig aliases resolve the synthetic specifier. Bare-package mocks paired with a project-root `__mocks__/<pkg>.ts` and Jest's `jest.mock(...)` are intentionally out of scope. Thanks [@boroth](https://github.com/boroth) for the report. (Closes [#251](https://github.com/fallow-rs/fallow/issues/251))

### Fixed

- **Angular 14+ `inject()` field-initializer DI is recognized.** Class fields written `private readonly inner = inject(InnerService)` (or `inject<InnerService>()`) now register `this.inner -> InnerService` so any `this.inner.member` chain credits `member` as used on `InnerService`. Previously every member of an `inject()`-acquired service consumed only via the field chain was reported as `unused-class-member`; the legacy constructor-parameter form was unaffected. The `inject` callee is gated by a named-import check against `@angular/core`, so a same-named `inject` from any other module is intentionally ignored. Thanks [@OmerGronich](https://github.com/OmerGronich) for the report. (Closes [#244](https://github.com/fallow-rs/fallow/issues/244))
- **Playwright POM fixture members consumed only through typed `base.extend<T>(...)` definitions are now credited.** Methods on a Page Object Model class that are referenced exclusively from a Playwright `test('name', async ({ adminPage }) => { adminPage.method() })` callback no longer surface as `unused-class-members`. Fixture definitions accept a named type alias (`type MyFixtures = { adminPage: AdminPage }`), an inline type literal, or any intersection / parenthesized form thereof; the analyzer correlates each typed fixture with the matching callback-side member access and credits the method on the defining POM class. The `base.extend` callee is gated against `@playwright/test`-named imports so the same identifier from any other module is intentionally ignored. Thanks [@vethman](https://github.com/vethman) for the report. (Closes [#246](https://github.com/fallow-rs/fallow/issues/246))

## [2.60.0] - 2026-05-01

### Added

- **`includeEntryExports` config option, and `--include-entry-exports` is now a global CLI flag.** Set `"includeEntryExports": true` (JSON / JSONC) or `includeEntryExports = true` (TOML) in your fallow config to opt in to entry-file export validation persistently, without passing `--include-entry-exports` on every run. The flag is now accepted in combined mode (`fallow --include-entry-exports`) as well as on `fallow dead-code` and `fallow audit`; previously the bare combined invocation rejected the flag because it was only defined on the `dead-code` subcommand. The `audit` MCP tool also gains an `include_entry_exports` param to match `analyze` / `check_changed`. Thanks [@filipw01](https://github.com/filipw01) for the report. (Closes [#249](https://github.com/fallow-rs/fallow/issues/249))

### Fixed

- **SCSS / Sass `@use 'X'` no longer resolves to a sibling `X.tsx`.** When both `Widget.scss` and `Widget.tsx` exist next to each other and a `.scss` importer does `@use 'Widget'`, fallow now resolves the import to `Widget.scss` per Sass's actual resolution algorithm. The standard module resolver's extension list contains `.tsx` / `.ts` before `.scss`, so without this guard a bare specifier from a stylesheet was silently picking the JS/TS sibling, creating phantom circular dependencies in CSS-modules / Angular `styleUrls` patterns where the `.tsx` component imports its own `.scss` and a sibling `.scss` shares variables/mixins. Stylesheet importers now reject any standard-resolver hit whose extension is a JS/TS-family extension (`.tsx`, `.ts`, `.mts`, `.cts`, `.js`, `.jsx`, `.mjs`, `.cjs`) and re-route through the SCSS-aware fallback chain (CSS-extension probe, `_filename` partial convention, framework include paths, `node_modules` walk-up); when those also fail, the import is reported as unresolved instead of falling through to JS/TS extensions. Thanks [@OmerGronich](https://github.com/OmerGronich) for the precise reproduction and the suggested fix. (Closes [#245](https://github.com/fallow-rs/fallow/issues/245))

## [2.59.0] - 2026-05-01

### Added

- **Persistent token cache for `fallow dupes`.** Projects above `duplicates.minCorpusSizeForTokenCache` (default `5000` source files) reuse tokenized output across runs under `<root>/.fallow/cache/dupes-tokens-vN/`. Below the threshold the cache stays disabled because load/save overhead exceeds the tokenize savings. Disable explicitly with `--no-cache`.
- **Shingle prefilter for focused-mode duplication.** When `--changed-since` is set on a project at or above `duplicates.minCorpusSizeForShingleFilter` (default `1024` files), the detector drops unchanged files whose k-token shingles do not overlap any changed file before building the suffix array. This is the dominant speed-up on large monorepos with small diffs.
- **`duplicates.ignoreDefaults` config + `--explain-skipped` global flag.** Opt-out for the new built-in ignores below; `--explain-skipped` expands the human/markdown skipped-file note into per-pattern counts.

### Changed

- **`fallow dupes` skips generated framework output by default.** Duplication analysis now ignores `**/.next/**`, `**/.nuxt/**`, `**/.svelte-kit/**`, `**/.turbo/**`, `**/.parcel-cache/**`, `**/.vite/**`, `**/.cache/**`, `**/out/**`, and `**/storybook-static/**` before tokenization, while keeping authored-looking `lib/`, `legacy/`, and nested `build/` directories in scope. These defaults merge with `duplicates.ignore`; set `duplicates.ignoreDefaults: false` to opt out and use only your configured ignore list. If your duplication number drops on upgrade, it is because fallow is now excluding generated framework output from duplicate detection by default. Human and markdown output show a one-line skipped-file note, and `--explain-skipped` expands it to per-pattern counts; JSON, SARIF, CodeClimate, and compact output stay unchanged.
- **`fallow init` scaffolds a commented-out `[duplicates]` block.** The generated `.fallowrc.json` (now valid JSONC end-to-end) and `fallow.toml` ship with example `ignore` additions for `lib/`, `legacy/`, `__generated__/`, and `generated/` directories that vary per project.
- **`--changed-since` wires straight into the focused fast path.** Resolving the changed-file set up front lets the engine engage the shingle prefilter and extraction-time interval pruning instead of running a full-corpus scan followed by a redundant post-filter. The audit driver also skips the base-snapshot pass when `--gate all` is set, because attribution is irrelevant to the verdict in that mode. Also fixes a latent `IntervalIndex` coalescing bug in post-LCP filtering that was fragmenting intervals and keeping more groups than necessary. Thanks [@OmerGronich](https://github.com/OmerGronich) for the deep profiling on MUI that surfaced both this fix and the new default-ignore set below. (Closes [#243](https://github.com/fallow-rs/fallow/issues/243))
### Fixed

- **Rayon worker stack pinned to 16 MiB.** Deep AST visitor and graph traversals could overflow Rust's default 8 MiB worker stack on very large real-world projects (e.g. `microsoft/TypeScript`), aborting `dead-code` and `health` analysis with `fatal runtime error: stack overflow`. The CLI and NAPI entry points now share a single rayon pool builder that pins worker stack size to 16 MiB and agrees on thread-count defaults, so the previous `RUST_MIN_STACK=16777216` workaround is no longer needed and embedders see consistent parallel behaviour. Thanks [@OmerGronich](https://github.com/OmerGronich) for the report. (Closes [#247](https://github.com/fallow-rs/fallow/issues/247))

## [2.58.0] - 2026-05-01

### Added

- **`fallow hooks install --target {git,agent}` and `fallow hooks uninstall --target {git,agent}` namespace.** Single command surface for both the shell-level Git pre-commit hook scaffolder and the Claude Code / Codex agent gate. `fallow init --hooks` and `fallow setup-hooks` continue to work as compatibility aliases that delegate to the same engine. `install` accepts `--branch`, `--agent`, `--dry-run`, `--force`, `--user`, and `--gitignore-claude`; `uninstall` reuses `--target`, `--agent`, `--dry-run`, `--force`, and `--user`. A `# Generated by fallow hooks install --target git.` marker on Git hook scripts lets `uninstall` preserve user-authored pre-commit scripts unless `--force` is passed.
- **`fallow coverage upload-source-maps` CI subcommand.** Scans a build output directory for `.map` files and uploads them to `/v1/coverage/{repo}/source-maps` so cloud-mode runtime coverage can resolve bundled paths back to original sources. Flags: `--dir` (default `dist`, scanned recursively), `--include` / `--exclude` (glob patterns), `--repo` (auto-detected from `package.json` `repository.url` or `git remote get-url origin`), `--git-sha` (auto: `$GITHUB_SHA` → `$CI_COMMIT_SHA` → `$COMMIT_SHA` → `git rev-parse HEAD`), `--endpoint`, `--strip-path` (default `true`; emit basename as `fileName`, set `false` to keep dir-relative paths for monorepo bundlers), `--dry-run`, `--concurrency` (default 4), `--fail-fast`. The API key is read only from `$FALLOW_API_KEY` (no flag form, intentional, keeps the secret out of argv). Per-map retry on transient `429`/`5xx`. Maps over 10 MiB warn; over 100 MiB are rejected. Exit codes: 0 ok · 1 partial-failure · 2 validation.

### Changed

- **Generated Git pre-commit hook resolves the base ref at commit time via `git merge-base @{upstream} HEAD`.** Previously the hook baked the auto-detected default branch in literally at install time, which broke on repos with multiple long-lived integration branches (next-release / hotfix / LTS) where the right base depends on which branch the feature targets. The new resolution diffs against the merge-base of the current branch's upstream and `HEAD`, so feature branches forked off a non-default integration branch compare against the actual fork point, not against their own remote tracking branch. `--branch` is repurposed as the fallback used only when no upstream is set. Existing on-disk hook scripts are unchanged; the new behaviour applies after re-running `fallow init --hooks` or `fallow hooks install --target git`. Thanks [@OmerGronich](https://github.com/OmerGronich) for the report. (Closes [#242](https://github.com/fallow-rs/fallow/issues/242))

## [2.57.0] - 2026-04-30

### Added

- **`fallow coverage analyze` runtime coverage subcommand.** Focused entry point alongside `coverage setup` and `coverage upload-inventory`. Local mode (`--runtime-coverage <path>`) reads a V8 coverage directory, V8 JSON file, or Istanbul `coverage-final.json`. Cloud mode is explicit opt-in only via `--cloud` / `--runtime-coverage-cloud` / `FALLOW_RUNTIME_COVERAGE_SOURCE=cloud`, fetches `/v1/coverage/{repo}/runtime-context` from fallow cloud, and merges runtime facts with the local AST/static analysis. `FALLOW_API_KEY` alone never selects cloud mode; ambiguous combinations are rejected. JSON output adds `summary.data_source` (`local`/`cloud`), `summary.last_received_at`, `summary.capture_quality`, kebab-case action identifiers (`delete-cold-code` / `review-runtime`), and a `cloud_functions_unmatched` warning when cloud sees functions absent from the local AST. `--explain` attaches a top-level `_meta` block with field definitions, enum vocabularies, and warning-code documentation.
- **`fallow explain <issue-type>` standalone rule explainer.** Prints a rule's rationale, worked example, fix guidance, and docs URL without running an analysis. Accepts shorthand (`unused-export`, `code-duplication`) and the canonical rule id (`fallow/unused-export`). JSON output is stable (`id`, `name`, `summary`, `rationale`, `example`, `how_to_fix`, `docs`); MCP exposes the same data via the `fallow_explain` tool.
- **`fallow audit --gate new-only|all`.** Default `new-only` gates only findings introduced by the changeset and reports inherited findings as context with a JSON `attribution` block plus per-finding `introduced: true|false`. `--gate all` (or `[audit] gate = "all"` in TOML, `audit.gate` in JSON) fails on every finding in changed files and skips the extra base-snapshot attribution pass.
- **Workspace-aware `fallow coverage setup --json`.** Workspace projects emit a per-runtime-package `members[]` array, union `runtime_targets`, and prefix member file paths; pure aggregator roots and build-only library packages are filtered out. Adds four paid runtime-context MCP tools (`get_blast_radius`, `get_importance`, `get_cleanup_candidates`, `check_runtime_coverage`) backed by the same fallow-cloud endpoint.
- **`.fallowrc.jsonc` discovered alongside `.fallowrc.json`.** First-match-wins discovery accepts a JSON-with-comments file as a peer of `.fallowrc.json`. CLI `--config` accepts the new extension; the discovery helper, init scaffolding, and config docs cover it. Thanks [@filipw01](https://github.com/filipw01) for reporting that `ignoreDependencies` was silently ignored when set in a `.fallowrc.jsonc` file. (Closes [#231](https://github.com/fallow-rs/fallow/issues/231))

### Fixed

- **Class members typed with a nullable annotation now bind correctly.** `class Foo { bar: Baz | null }` binds `Baz` for member-usage tracking; this eliminates false `unused-class-member` findings on type-annotated nullable fields. Thanks [@pedrobgodoy](https://github.com/pedrobgodoy). ([#233](https://github.com/fallow-rs/fallow/pull/233))
- **`--changed-since` git invocations no longer inherit `GIT_DIR`.** Prevents resolving against the wrong repository when `GIT_DIR` is set in the environment. The `fallow_explain` MCP tool name and `ExplainOutput` schema are corrected.
- **Single local V8/Istanbul capture works without a license.** Restores the documented free-tier behaviour; continuous/cloud monitoring still requires a license.
- **Nested barrel re-exports across workspace packages.** Re-export chains are now followed transitively through chained barrel files (e.g. `packages/app` importing a symbol from `packages/ui` via `foo/index.ts` → `foo/bar/index.ts` → `foo/bar/baz/index.ts` → the leaf module), so symbols re-exported through deep barrel structures are no longer flagged as unused. Thanks [@filipw01](https://github.com/filipw01) for the StackBlitz repro. (Closes [#236](https://github.com/fallow-rs/fallow/issues/236))
- **Runtime-coverage feature-gated build is clippy-clean** and several `coverage setup` rough edges (workspace recipe rendering, sidecar discovery messaging) are smoothed over.
- **`coverage analyze --cloud`:**
  - Switched to `Accept-Encoding: identity` (the bundled ureq build does not include the gzip feature); the cloud body now parses on production responses.
  - Empty-window responses no longer emit duplicate `no_runtime_data` warnings; the CLI defers to the server's variant.
  - Markdown / compact / sarif / codeclimate / badge `--format` values are rejected with an actionable error instead of silently emitting JSON.
  - HTTP 400 from the cloud endpoint maps to exit 2 (validation), not exit 7 (network).
  - The 404 catch-all branch no longer attributes routing 404s to "Repo not accessible to your org"; only `code: "repo_not_found"` produces that message.
  - `runtime_coverage.findings[].id` and `hot_paths[].id` are 8 hex chars, matching the published JSON schema.

### Changed

- MCP runtime-coverage tool descriptions reflect the single-capture-free contract.
- CI enforces conventional commit messages.

## [2.56.0] - 2026-04-30

### Added

- **`fallow fix --yes` removes unused exported enum declarations entirely.** When an exported `enum` is unused outside its file, the whole `export enum Foo { ... }` block is dropped instead of leaving a non-exported zombie declaration. Covers single-line, multi-line, `export const enum`, and `export declare enum` forms, with safe descending-index ordering when other export-list lines on the same file also get rewritten. Falls back to keyword-only stripping when the enum is still referenced locally so the file continues to compile. The remove-export action description in JSON output and the `fix_apply` MCP tool description are refreshed so consumers see the broader behaviour.
- **All-member enum-member removal folds into whole-enum removal.** When `fallow fix --yes` would remove every member of an exported enum, the entire `export enum X { ... }` block is dropped in a single write instead of stripping members one at a time and leaving behind a syntactically valid but semantically dead `export enum X {}` shell that detection then fails to surface on the next pass. JSON output collapses the per-member entries for a folded enum into a single `remove_export` entry at the declaration line so consumers see exactly the edit that happened. Non-JSON output emits a stderr advisory pointing at the TypeScript build for importer cleanup, since fallow's reachability detector cannot surface a name-level dangling import once the file resolves successfully. Thanks [@mjfwebb](https://github.com/mjfwebb) for reporting. (Closes [#232](https://github.com/fallow-rs/fallow/issues/232))
- **Tailwind v4 `@plugin` directive recognised in CSS, SCSS, and Vue/Svelte `<style>` blocks.** `@plugin "..."` is parsed as a default import, so package plugins (`daisyui`, `@tailwindcss/typography`) are credited as used dependencies and relative plugin files have their default export marked used. Unlike SCSS `@use`, extensionless package names stay bare in `.scss`/`.sass` files because they are package specifiers, not local partials. Verified against `daisyui`'s own `packages/playground`: without the fix, `daisyui` is flagged as an unused dev dependency; with the fix, it is correctly credited via `@plugin "daisyui"`. Thanks [@filipw01](https://github.com/filipw01) for reporting. (Closes [#230](https://github.com/fallow-rs/fallow/issues/230))

### Changed

- **Workspace packages treated as first-class dependencies.** Workspace package names are now checked the same way as external npm packages: resolved internal-module imports whose specifier matches a workspace name are credited to package usage, so monorepo edges between workspace packages count as usage. Declared-but-unimported workspace deps now surface in `unused_dependencies`, and imports of a workspace package from a workspace whose own `package.json` does not list it surface in `unlisted_dependencies`. Self-references stay allowed without requiring a package to depend on itself. External plugin definitions (`fallow-plugin-*` / inline framework configs) now also activate per-workspace inside `run_workspace_fast`, so a custom plugin enabled by a workspace dependency keeps its `alwaysUsed` and `toolingDependencies` honored during real analysis instead of only appearing in `fallow list --plugins`. Verified against `vue-core` (real pnpm monorepo): the change surfaces 3 new unused-dependency findings (incl. `@vue/reactivity` declared but only referenced via `declare module` in `runtime-dom`) and 11 new unlisted-dependency findings on `@vue/*` internal packages. Thanks [@filipw01](https://github.com/filipw01) for reporting. (Closes [#228](https://github.com/fallow-rs/fallow/issues/228))

### Fixed

- **Enum-member fix dedup keys on full triple so single-line folds fire.** The sort + dedup of `EnumMemberFix` previously used `line_idx` alone, which collapsed distinct members of a single-line enum (`export enum E { A, B }`) down to one entry before `detect_folded_enums` could see them. The fold's all-members-removed check then concluded the body still held member `B` and refused to fire, leaving a half-stripped one-liner instead of the whole-block delete the fold path expects. Sort and dedup now use `(line_idx desc, parent, member)`, so same-line distinct members survive into the fold pass.

### Internal

- Enum-declaration helpers (`declares_exported_enum`, `find_enum_declaration_range`, `removable_exported_enum_range`, supporting identifier-boundary helpers) extracted from `fix/exports.rs` into a new `fix/enum_helpers.rs` sibling module with `pub(super)` visibility, so both `exports.rs` and `enum_members.rs` can share them without re-implementation. No behaviour change.

## [2.55.0] - 2026-04-29

### Added

- **`private-type-leak` issue type for exported function and method signatures.** Detects when an exported `function`, class method, or class field references a type that is not also exported from the same module, producing a finding the consumer cannot name when they need to type a wrapper, mock, or destructured argument. Opt-in via the `private-type-leaks` rule (`rules.private-type-leaks: "error"` or `"warn"`); default severity is `"off"` so existing projects don't see new findings without explicit consent. Framework routing convention files (Next.js `app/` route handlers, Nuxt `pages/`, SvelteKit `+page.ts`/`+layout.ts`, Remix routes, TanStack route configs, Astro pages, Solid/Qwik routes) are skipped because their exports are framework contracts rather than public API surface.
- **`ignoreExportsUsedInFile` config option (knip parity).** When set to `true`, exports that are referenced inside the same file as their declaration are not reported as unused, matching knip's behavior for projects that re-export internal utilities purely for convenience or testing. Default `false` keeps the stricter fallow behavior.
- **TanStack virtual route configs supported.** The TanStack Router plugin now recognises virtual route files declared via `route(...)` / `index(...)` / `layout(...)` in a config, so route components consumed only by `routeTree.gen.ts` are no longer reported as unused. Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza). (#223)

### Changed

- **`fallow health --score` no longer auto-runs churn-backed hotspot analysis.** Plain `--score` now computes the score using duplication, dead-code, complexity, maintainability, unused-deps, circular-deps, unit-size, and coupling penalties. The `hotspots` penalty is only included when hotspot analysis runs (via `--hotspots`, or `--targets` with `--score`). The previous behavior forced every `--score` invocation to run a `git log` shell-out, which dominated health timing on large repos. Snapshot (`--save-snapshot`) and trend (`--trend`) flows still trigger hotspot vital signs so saved snapshots remain complete. Score numbers can rise on projects that previously took a non-zero hotspot penalty; CI `--min-score` gates may need re-baselining. The human output now hints `N/A: hotspots (enable the corresponding analysis flags)` and the JSON `health_score.penalties.hotspots` field is omitted when the penalty was not computed.

### Fixed

- **LSP server now serves document diagnostic pulls.** Editors that follow the `textDocument/diagnostic` pull model (modern Helix, Zed, recent Neovim configs) previously got no diagnostics from the fallow LSP because the server only published via the push channel. The pull handler now returns a `RelatedFullDocumentDiagnosticReport` keyed by the requested document's URI, so pull-mode and push-mode editors see the same findings.

### Performance

- **Audit shares dead-code parse and analysis with health and dupes.** `fallow audit` previously ran the dead-code pipeline (parse, semantic, graph build, plugin pass) once for `check`, again for `health`, and again for `dupes`. The orchestrator now computes the shared result once and threads the file list and analysis output through to both downstream steps, cutting redundant parsing work in the dominant local-developer flow.
- **Workspace plugin runs see only their own files.** `run_plugins` now buckets discovered files by workspace root before parallel plugin execution and passes each workspace its own pre-computed relative paths, instead of feeding every workspace the full project file list. Cuts the per-workspace plugin matcher work from `O(workspaces × all_files × matchers)` to `O(workspace_files × matchers)` on monorepos. End-to-end plugin detection is unchanged because the filesystem-fallback Phase 3b still scans workspace and project roots for unmatched config files.
- **Duplicate-export importer overlap is single-pass.** `find_duplicate_exports` pre-builds an `FxHashMap<&Path, FileId>` index instead of scanning every module per location, and `has_common_importer` walks each duplicate file's `reverse_deps` once into an `importer_owner` map instead of comparing every pair of importer sets. Same output, lower complexity on projects with many duplicate-export groups.

### Internal

- New regression test asserts `expand_recursive_workspace_pattern` preserves nested workspace roots (e.g., `apps/app/packages/nested/`) when both parent and child have a `package.json`.
- New end-to-end test asserts `run_workspace_fast` invokes `plugin.resolve_config()` for workspace-local config files and surfaces the parsed entry pattern.
- Pre-push hook now unsets `GIT_DIR` and `GIT_WORK_TREE` so integration tests run from the worktree path even when the user's shell environment exports those variables (fixes flaky push from worktrees with bare-repo-style configs).
- `.gitignore` updated to exclude crash handover marker files.
- Dependency bump: `tokio` 1.51.1 → 1.52.1 (#218).
- Docs touch-ups: MCP `analyze` tool description, agent context note for the new opt-in default of the `private-type-leaks` rule, and `crap_max` description refined across coverage_model variants.

## [2.54.3] - 2026-04-29

### Fixed

- **Class members are credited through cross-file exported instances.** When an instance is created in one file (`export const box = new Box()`) and consumed in another (`box.bump()`), the visitor now records a synthetic `MemberAccess` entry tying each instance export to its source class. The analysis layer reads those entries to build an instance-export to class-export-key map, walks through re-export chains, and propagates accumulated `accessed_members` from each instance key onto its target class key. Previously `Box.bump` (and getters/setters) were reported as unused with `auto_fixable: true`, which would let `fallow fix` silently delete methods in active use. Sentinel-prefixed accesses are filtered out of the main accessed-members loop and the heritage chain extractor so they do not leak into unrelated detection paths. (Closes [#222](https://github.com/fallow-rs/fallow/issues/222))
- **Class members accessed via nested injected-dependency object types are credited.** Members reached through typed nested object bindings such as `constructor(private deps: { foo: FooClass }) { ... } this.deps.foo.foo()` were previously reported as unused. The visitor now walks `TSTypeLiteral` nodes to register dotted-path bindings (`this.deps.foo -> FooClass`), records full-chain object names in `visit_static_member_expression`, and propagates nested bindings through explicit `this.x = y` assignments for the no-accessibility-modifier form.

### Internal

- Cache version bumped 54 → 55 so warm caches re-extract the new instance-binding shape.

## [2.54.2] - 2026-04-28

### Fixed

- **Svelte template scanner now credits identifiers used in ternary branches and inline spread objects.** A double-increment bug in `scan_curly_section` caused the brace-section reader to skip the closing quote of an empty quoted string (`''` / `""`), leaving the in-quote flag stuck on, returning `None`, and aborting the rest of the template via the outer loop's `break`. Any expression inside or after `{cond ? expr : ''}` was therefore invisible to the scanner. The first byte of the brace was already incremented by the loop's terminal `index += 1`, so removing the in-arm increment fixes empty-string handling without changing any other path. Additionally, markup-tag braces now process `{...spread}` rest-attributes (and any non-`@attach` brace) instead of dropping them, so `<button {...{ "data-x": inSpread() }}>` correctly marks `inSpread` as used. Cache version bumped 53 → 54 so warm caches re-extract the new template-usage shape.

### Internal

- Bash test helpers (`ci/tests/run.sh`, `action/tests/run.sh`) replaced their `echo "$output" | grep -q ...` `assert_contains` / `assert_not_contains` implementations with bash native `[[ "$output" == *"$expected"* ]]` matching. The pipeline form was prone to `SIGPIPE` under `set -o pipefail` when grep matched early and closed the pipe before echo finished, intermittently failing CI with a misleading "expected to contain: ..." message on whichever assertion happened to ride the race.
- VS Code extension `engines.vscode` bumped to `^1.116.0` to align with `@types/vscode@1.116.0`; vsce had been rejecting the package with "@types/vscode greater than engines.vscode".
- Dependency bumps: `clap` 4.6.0 → 4.6.1, `rmcp` 1.4.0 → 1.5.0, `srcmap-sourcemap` 0.3.5 → 0.3.6, the `oxc` group (8 crates), plus VS Code dev-dependency refreshes (`@types/vscode`, `@vscode/vsce`, `rolldown`, `typescript`, `vitest`) and CI action bumps (`actions/setup-node`, `dependabot/fetch-metadata`).

## [2.54.1] - 2026-04-28

### Fixed

- **`fallow-cli` now publishes to crates.io.** v2.54.0 introduced `crates/cli/src/ci_template.rs` with `include_str!` paths (`"../../../ci/gitlab-ci.yml"` and 13 others) that resolved during workspace builds but failed during `cargo package` because the published tarball only contains `crates/cli/`, not the workspace's `ci/` and `action/` directories. The publish step's verification compile failed silently behind a `|| echo "Already published, skipping"` mask, so v2.54.0 of `fallow-cli` never reached crates.io while the eight upstream crates published cleanly. The 14 bundled files now live under `crates/cli/templates/` so `include_str!` resolves inside the crate, and a new `bundled_templates_match_workspace_sources` unit test asserts byte-equivalence between the bundled copies and the canonical workspace sources at `<root>/ci/` and `<root>/action/`. Drift between the two is caught by `cargo test` against the workspace before the next tag.

## [2.54.0] - 2026-04-28

### Added

- **`fallow ci-template gitlab` subcommand for offline-runner pipelines.** `fallow ci-template gitlab` prints the bundled `ci/gitlab-ci.yml` to stdout. `fallow ci-template gitlab --vendor [DIR]` writes the template plus 13 jq/bash helper files (4 `ci/jq/`, 2 `ci/scripts/`, 7 `action/jq/`) under DIR (default `.`), letting GitLab pipelines run the full MR-comments + Code Quality + review integration without reaching `raw.githubusercontent.com` at pipeline runtime. `--force` is required to overwrite files that diverge from the bundled template; refusing to overwrite is the default to protect user edits.
- **`--format gitlab-codequality` and `--format gitlab-code-quality` aliases for `codeclimate`.** The CodeClimate JSON array format is unchanged; the new aliases just make the GitLab Code Quality use case discoverable via the name GitLab consumers actually reach for. Aliases work via `--format`, `FALLOW_FORMAT`, and `fallow schema`.
- **`fallow health --sort severity`** orders complexity findings by exceeded-priority + severity rather than by cyclomatic complexity. Opt-in: the CLI default stays `cyclomatic` to avoid breaking JSON consumers that assume cyclomatic ordering. The PR/MR jq scripts use the same composite-key sort independently, so the default does not affect comment rendering. Available across the CLI, the NAPI bindings, and the MCP server.

### Changed

- **GitHub Action and GitLab CI PR/MR complexity tables widened from 4 to 7 columns.** The complexity table in PR/MR comments now shows Severity, Cyclomatic, Cognitive, CRAP, Lines, plus per-cell `**!**` markers identifying which thresholds each function exceeded. The footnote spells out the active thresholds with explicit `default` fallbacks so older fallow JSON renders cleanly. CRAP-only findings (functions flagged purely by CRAP=30 with no runtime coverage) now stay visible even when several cyclomatic flags exist; previously they were buried below the cyclomatic-sorted top-5 cut. The CRAP column auto-hides when the JSON lacks CRAP metadata, preserving back-compat with older fallow versions.
- **GitLab CI template: `GIT_STRATEGY: "fetch"` and `GIT_DEPTH: "0"` set as defaults** so shared organisation templates that set `GIT_STRATEGY=none` or a shallow clone do not strand fallow without a working tree or without enough history to diff against the MR base SHA.

### Fixed

- **GitHub Action now respects package.json fallow pins.** The action's default `version: latest` previously installed whatever npm tagged latest, ignoring the project's `package.json` `fallow` pin. Local `pnpm exec fallow` could run 2.7.3 while CI ran 2.52.2, producing wildly divergent reports. The `version` input default is now empty; when omitted, `install.sh` reads the project package.json `fallow` dependency and installs that, falling back to `latest` only when no pin is found. The new spec whitelist accepts semver versions and ranges (`^2.52.0`, `>=2.0.0 <3.0.0`) and rejects `file:`, `link:`, `workspace:`, git URLs, paths, and `2.0.0 -g malicious` style flag injection. A drift warning fires when an exact pin diverges from the installed version (suppressed when `version:` was set explicitly). `INPUT_ROOT` is plumbed so monorepos with non-root `package.json` work.
- **GitLab CI now respects package.json fallow pins to match GitHub Action behavior.** When `FALLOW_VERSION` is empty, the GitLab template reads the project's `package.json` `fallow` dependency, validates the spec with the same injection-safe whitelist as the GitHub Action, supports semver ranges, falls back to `latest` for unsupported package specs, and warns when an exact package pin drifts from the installed CLI version. The default value of `FALLOW_VERSION` changed from `"latest"` to `""`; pipelines that omit `FALLOW_VERSION` and have a `fallow` entry in `package.json` will now install that pinned version on the next run instead of `latest`. Set `FALLOW_VERSION: "latest"` explicitly to keep the previous behaviour.
- **GitLab CI: `comment.sh` and `review.sh` hard-require `GITLAB_TOKEN`.** GitLab's documented `CI_JOB_TOKEN` permissions allow reading MR notes, but not creating, updating, or deleting them; the previous fallback was non-functional in practice and produced confusing 401/403 errors at API call time. The scripts now exit `0` with a clear stdout warning when `GITLAB_TOKEN` is unset rather than blocking the pipeline. `CI_JOB_TOKEN` is still useful for GitLab package registry authentication.

## [2.53.0] - 2026-04-28

### Added

- **Cross-workspace dependency leak detection.** `fallow dead-code` now flags packages declared in one workspace's `package.json` but only consumed from another workspace's source files, surfaced as a new `cross-workspace-dependency` issue with a `move-dependency` action carrying the suggested target workspace path. Helps monorepo maintainers find dependencies that drifted out of their original workspace as code moved around. Available across every output format (human, JSON, SARIF, CodeClimate, markdown, compact); the JSON action `type` enum gains `move-dependency`.

### Fixed

- **Vue script-instance member access in templates is now credited.** `<script setup> const counter = new Counter(); ...` followed by `<button @click="counter.bump()">{{ counter.value }}</button>` no longer reports `Counter.bump` and `Counter.value` as unused class members. The Vue template scanner threads the script block's instance-binding map through expression and statement evaluation so `counter.bump()` remaps to `Counter.bump`. v-for / v-slot locals continue to shadow the script binding, avoiding false positives. Mirrors the same fix shipped earlier in this cycle for Svelte. Cache version bumped to 53 so existing caches re-extract the new member-access shape.
- **Svelte 5 `{@attach}` directives and arrow-bound class member calls are now credited as used.** `<div {@attach myAttach}>` no longer reports `myAttach` as an unused export, and `onclick={() => counter.bump()}` plus `{counter.value}` correctly map to the script-local `Counter` instance's `bump` and `value` members through the script's binding-target table. Template locals (e.g., `{#each rows as counter}`) shadow the script binding so genuinely unused names in shadowed scopes still get reported. A small follow-up skips synthetic `this.*` keys when seeding the template scanner's bound targets, trimming dead weight from the reference set. Closes [#200](https://github.com/fallow-rs/fallow/issues/200). Thanks [@kevmodrome](https://github.com/kevmodrome) for the report and [@imwyvern](https://github.com/imwyvern) for [#201](https://github.com/fallow-rs/fallow/pull/201).
- **Peer dependencies of used packages are no longer reported as unused.** `fallow dead-code` now reads installed packages' required `peerDependencies` and credits them when the package itself is used, recursively. Peers marked optional via `peerDependenciesMeta.<name>.optional: true` are still reported when otherwise unused. Package lookup follows ancestor `node_modules` directories so workspace-local, hoisted monorepo, and scoped-package installs are covered. Closes [#199](https://github.com/fallow-rs/fallow/issues/199).
- **Linux GNU release binaries now run on glibc 2.31+ (Debian Bullseye, Ubuntu 20.04, RHEL 9, Amazon Linux 2023).** Previous releases inherited the GitHub `ubuntu-latest` runner's glibc 2.39, which broke fallow on most CI images and older distros. The release workflow now compiles `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` inside the `rust:1.95-bullseye` container, with a post-build `readelf` gate that fails the release if any shipped binary requires a glibc version newer than 2.31. Forward-only fix: applies to artifacts produced from this tag onward. Closes [#191](https://github.com/fallow-rs/fallow/issues/191). Thanks [@OmerGronich](https://github.com/OmerGronich).
- **`docs/output-schema.json` now lists `move-dependency` in the action `type` enum.** New emit sites in `crates/cli/src/report/json.rs` introduced a value the schema didn't know about, which would have rejected every cross-workspace finding for consumers validating against the published schema.

## [2.52.2] - 2026-04-28

### Fixed

- **Workspace-aware unused-dependency baselines.** When the same package appeared as unused in multiple workspace `package.json` files, `fallow dead-code --save-baseline` collapsed them into a single bare-package-name key. Suppressing the entry in one workspace then silenced every other workspace's report of the same package, hiding genuinely unused deps in the rest of the monorepo. Baseline keys for `unused_dependencies`, `unused_dev_dependencies`, `unused_optional_dependencies`, `type_only_dependencies`, and `test_only_dependencies` are now `package.json:package_name` so per-workspace instances stay distinct. Existing baselines saved with bare-package-name keys still match on load, so upgrading does not require regenerating baselines.

## [2.52.1] - 2026-04-27

### Fixed

- **Stack overflow on Svelte snippet params with typed tuple bindings.** `extract_pattern_binding_names` and `collect_pattern_usage` only stripped trailing TS type annotations when the pattern started with `{` or `[`. A snippet param like `{#snippet foo(x: [number, number])}` reached the comma-split branch where `split_top_level` refused to split (commas at depth=1 inside the tuple) and recursed on the unchanged input until the stack overflowed. Now top-level commas split first, then a top-level `:` scan strips trailing type annotations for both destructured and plain identifier bindings. Closes [#172](https://github.com/fallow-rs/fallow/issues/172).
- **ESLint config: dependencies inside `overrides[*]` and relative `extends` chains are now detected.** Two parser gaps produced false-positive unused devDependency reports: `parser`/`plugins`/`extends` inside `overrides[*]` were ignored (only top-level fields were extracted), and `extends` entries pointing at relative paths (`./config/base.js`, `../shared/eslintrc.json`) were passed verbatim to the package-name resolver, producing nonsense like `eslint-config-./config/base.js`. The walker now recurses into each override entry as a sub-config and resolves path-like extends relative to the parent dir, probing `.js`/`.cjs`/`.mjs`/`.json` like ESLint's resolver. Cycle protection via `FxHashSet<PathBuf>`; depth bounded at 8. Closes [#198](https://github.com/fallow-rs/fallow/issues/198).
- **`ignorePatterns` is now honored when warning about undeclared workspaces.** Directories whose project-root-relative path matched the user's `ignorePatterns` config still triggered the `package.json directory not declared as a workspace` warning. The compiled `GlobSet` from `ResolvedConfig` is now plumbed into `find_undeclared_workspaces` and matched against both the relative directory path and its `package.json` suffix, so paths like `references/vitest` are silenced consistently with the rest of the analysis. Closes [#193](https://github.com/fallow-rs/fallow/issues/193).
- **Five false-positive cases for files referenced from non-source artifacts.** Vite `css.preprocessorOptions.{scss,sass,less,stylus}.additionalData` bodies are now scanned for `@use`/`@import`/`@forward`; local paths seed entry patterns and bare specifiers credit referenced dependencies. Vue/Svelte SFC `<style lang="scss">` body imports and `<style src="...">` references are extracted symmetrically with `<script src>` (imports tagged `from_style: true` so the resolver routes through CSS-restricted SCSS fallbacks before the standard resolver, avoiding `./Foo` resolving to the `.vue` self-file). `package.json` scripts positional file arguments now use the workspace prefix when joined into entry patterns with `..` segment normalization. CI YAML scanner returns `CiAnalysis { used_packages, entry_files }` so positional file paths in `.gitlab-ci.yml` and `.github/workflows/*.yml` are seeded as entries. Cypress plugin extracts `e2e.specPattern`, `component.specPattern`, `e2e.supportFile`, `component.supportFile` from `cypress.config.{ts,js,mjs,cjs}` so Cypress 10+ projects whose specs live outside `cypress/**` stay reachable. A follow-up commit covered additional Vite preprocessor and Cypress edge cases. Cache version bumped to 51 (new `from_style` field on `CachedImport`). Closes [#195](https://github.com/fallow-rs/fallow/issues/195).
- **LSP `changedSince` filter now works when the workspace is a subdirectory of the git repo.** Turborepo, Next.js `apps/`, and Nx `packages/` layouts silently produced an empty filter because `try_get_changed_files` joined `git diff` output (toplevel-relative) against the workspace root (subdirectory). The resolver now resolves the canonical git toplevel via `git rev-parse --show-toplevel`, joins git output against it, and passes `--full-name` to `git ls-files` for parity with `git diff`. The LSP canonicalizes the workspace root in `initialize` so absolute paths agree across symlink boundaries on macOS and Windows. A one-time WARN log fires when workspace differs from toplevel so monorepo subdirectory layouts are self-diagnosable. Same release also dedupes cross-root duplicates that `merge_results` accumulates when multiple project roots walk overlapping files (with `UnlistedDependency` getting a real merge of `imported_from` sites across roots), stamps `Diagnostic.data` with `{ "changedSince": "<ref>" }` when the filter is active so AI agents reading via `vscode.languages.getDiagnostics()` can verify the filter, and promotes the `(since <ref>)` indicator to all four status bar states via a shared `renderStatusBarText` helper. Closes [#190](https://github.com/fallow-rs/fallow/issues/190).
- **LSP advertises `diagnostic_provider` (LSP 3.17 pull-model) so strict clients call `textDocument/diagnostic`.** The pull handler and `cached_diagnostics` map were already wired up, but `ServerCapabilities.diagnostic_provider` was unset. Strict 3.17 clients (Helix, Zed) gate the pull request on the advertised capability, so the handler was dead code for those editors; VS Code's LSP client tolerated the gap by falling back to push-mode. The advertisement uses `identifier: "fallow"`, `inter_file_dependencies: true` (cross-file findings such as unused exports / unused dependencies), and `workspace_diagnostics: false` (no `workspace/diagnostic` handler).
- **`cargo doc --document-private-items` no longer fails on a public-doc-links-private warning in `crates/extract/src/css.rs`.** The doc comment on `extract_css_imports` (public) referenced `[\`normalize_css_import_path\`]` (private), which `-D rustdoc::private-intra-doc-links` rejects. Reverted to plain backtick formatting; CI `cargo doc --workspace --no-deps --document-private-items` is green again. Pre-existing problem; no behavior change.
- **`zizmor` Actions Security scan no longer audits fallow's own test fixtures.** The CI step ran `uvx zizmor ... .` from the repo root, which swept in `tests/fixtures/issue-195-ci-file-args/.github/workflows/deploy.yml` (a deliberately lax fixture used by fallow's CI YAML detection, not a real workflow) and failed on `unpinned-uses` plus `excessive-permissions`. Scoped the invocation to `.github/` and `action.yml` so the audit covers the workflows we actually ship.

## [2.52.0] - 2026-04-26

### Added

- **`fallow dupes --group-by` partitions clone groups by owner / directory / package / section.** Previously the flag was validated (so global flag errors stayed consistent across `check`, `health`, and `dupes`) but the resolver was built and discarded; the output stayed ungrouped. Now grouped output mirrors the existing `health --group-by` shape across every format. Multi-owner clone groups are attributed to the **largest-owner** (most instances in the group) with **alphabetical tiebreak** on equal counts, matching jscpd's majority-instance attribution; per-instance owners are surfaced inline so consumers can see split ownership. JSON output gains `grouped_by`, `total_issues`, and a `groups` array carrying per-bucket `stats`, `clone_groups` (each with `primary_owner`), and `clone_families`. SARIF gains `properties.group` per result; CodeClimate gains a top-level `group` per issue. Human output mirrors health's grouped section style: cyan-bullet `Per-{mode} duplication` header, per-bucket cyan-bullet sub-headers with stats, and a project-totals footer. The largest-owner attribution preamble renders only for `--group-by owner` (where attribution is genuinely ambiguous); directory / package / section modes hide it as cognitive noise. `--top` is skipped when `--group-by` is active so per-bucket stats reflect the full bucket; the renderer applies its own per-bucket cap. The MCP `find_dupes` tool already plumbed `--group-by`; no MCP-side change needed.
- **`BoundaryZone.root` for monorepo per-package boundary configurations.** `[[boundaries.zones]] root = "packages/app/"` resolves the zone's `patterns` against paths relative to the subtree instead of the project root. At classification time, `classify_zone` checks the path starts with the root prefix; if yes, strips it and glob-matches the patterns against the remainder; if no, skips the zone. Filter approach over prefix-rewriting: zero-cost for zones without `root`, keeps stored pattern strings un-corrupted, debug output stays clean. Closes the `FALLOW-BOUNDARY-ROOT-RESERVED` warning ramp from v2.51.0; the warning emission and `BoundaryConfig::reserved_root_zones()` method are removed. A new `FALLOW-BOUNDARY-ROOT-REDUNDANT-PREFIX` validation flags patterns that double-prefix the root (e.g., `root: "packages/app/"` + `patterns: ["packages/app/src/**"]`) so users who manually pre-prefixed their patterns during the warning ramp see the migration cue. Validation is emitted via `tracing::error!` consistent with `validate_zone_references`. Root matching is case-sensitive, matching globset's pattern conventions; locked down with a regression test to prevent silent platform-divergent classification on case-insensitive filesystems. New ADR: `decisions/007-boundary-zone-root.md`.
- **`<template>` complexity findings now cover inline Angular `@Component({ template: \`...\` })` decorators.** v2.51.0 emitted synthetic `<template>` cyclomatic/cognitive findings only for standalone `.html` files referenced via `templateUrl`, leaving codebases that put their templates inline (the modern Angular default) with zero coverage for the same control-flow density. The visitor now records each captured inline `template:` literal and the parser runs the existing template-complexity scanner over it, anchoring the resulting finding at the host file's `@Component`/`@Directive` decorator line. Jump-to-source lands on the decorator and `// fallow-ignore-next-line complexity` placed directly above the decorator suppresses through the existing health-side check, no extra suppression plumbing required. Template literals containing `${...}` expressions and `template:` properties bound to a variable are skipped (out of scope for the first cut). Cache version bumped to 49 so existing caches re-extract the new finding shape. Closes [#187](https://github.com/fallow-rs/fallow/issues/187).

### Changed

- **Schema version policy documented explicitly in `docs/output-schema.json`.** The `SchemaVersion` definition now states the bump policy: additive changes (new optional top-level fields, new optional struct fields, new array entries, new MCP tools, new CLI flags that map to new optional fields) do NOT bump the version; breaking changes (renamed fields, removed fields, type changes, enum-variant removals, semantic changes to existing fields) DO bump. Consumers should feature-detect new fields via JSON-key existence rather than gating on the version number. Resolves the ambiguity around `dupes --group-by` adding `grouped_by` / `total_issues` / `groups` without a version bump.

### Fixed

- **`cargo doc --document-private-items` no longer fails on `<template>` and unresolved intra-doc links.** Three rustdoc errors landed in earlier v2.51.x work: a bare `<template>` HTML tag in a doc comment on `InlineTemplateFinding`, an unresolved `[`VitalSigns`]` / `[`HealthScore`]` intra-doc link in `crates/cli/src/health/grouping.rs`, and a public-doc-links-private warning on `[`crate::health::HealthResult`]` in `crates/cli/src/health_types/grouped.rs`. CI `cargo doc --workspace --no-deps --document-private-items` is green again. Pre-existing problem; no behavior change.

## [2.51.0] - 2026-04-26

### Added

- **Synthetic `<template>` complexity findings for Angular HTML templates.** `fallow health --complexity` now scans standalone `.html` files for Angular template syntax and emits a synthetic finding named `<template>` whenever the template uses control-flow blocks (`@if`/`@else`/`@for`/`@switch`/`@case`/`@defer (when ...)`/`@let`), legacy structural directives (`*ngIf`, `*ngFor`, `[ngFor]`, `[ngForOf]`), expression-bound attributes (`[x]`, `(x)`, `bind-x`, `on-x`), or `{{ }}` interpolations. Cyclomatic increments per branch; cognitive accumulates with nesting and resets logical-operator runs across ternary branches. The finding flows through every output format (human, JSON, SARIF, CodeClimate, markdown, compact) and feeds the existing health score, hotspot, and refactoring-target pipelines. Suppress with `<!-- fallow-ignore-file complexity -->` at the top of the template; the JSON `actions` array carries a `suppress-file` entry with the exact comment so AI agents can apply it. Closes [#183](https://github.com/fallow-rs/fallow/issues/183). The CRAP-coverage story for `<template>` findings (JIT inherit-from-`.ts`, AOT source-map back-mapping) is tracked separately in [#186](https://github.com/fallow-rs/fallow/issues/186).
- **Tier-aware CRAP refactoring actions and baseline-aware suppression.** `fallow health --complexity` now emits coverage-leaning actions tailored to the function's coverage tier when CRAP triggers a finding: `add-tests` for `coverage_tier: none`, `increase-coverage` for `partial`/`high`, and `refactor-function` when even 100% coverage cannot bring CRAP below the threshold. CRAP-only findings within five of the cyclomatic threshold also append a secondary `refactor-function` action when cognitive complexity passes the cognitive floor (suppresses false positives on flat type-tag dispatchers and JSX render maps). Baseline-active runs and projects with `health.suggestInlineSuppression: false` no longer emit `suppress-line` actions; the report root carries an `actions_meta: { suppression_hints_omitted: true, reason: "baseline-active" }` breadcrumb so consumers can detect the omission.

### Changed

- **`--production-coverage` renamed to `--runtime-coverage` (no backwards-compatibility alias).** V8 and Istanbul measure invocations at runtime regardless of environment, so "production coverage" implied a restriction that did not exist; the beacon ships from any environment the operator points it at (staging, dev, load test, production). Surface changes: CLI flag `--production-coverage` → `--runtime-coverage`; MCP tool `check_production_coverage` → `check_runtime_coverage`; JWT feature claim `production_coverage` → `runtime_coverage`; SARIF rule IDs `fallow/production-*` → `fallow/runtime-*` (subtypes `safe-to-delete`/`review-required`/`low-traffic`); Rust types `ProductionCoverage*` → `RuntimeCoverage*`; source files `health_types/production_coverage.rs` → `runtime_coverage.rs`; tests `production_coverage_tests.rs` → `runtime_coverage_tests.rs`; snapshots `*_with_production_coverage.snap` → `*_with_runtime_coverage.snap`. The unrelated `--production` static-analysis flag (excludes test/dev files) is unchanged; only the paid V8/Istanbul coverage feature is renamed. SARIF / CodeClimate consumers that filter by `ruleId` need to update their selectors. Counterpart changes ship in fallow-cloud (sidecar + dashboard) and the companion repos (fallow-docs, fallow-skills) so the protocol, server, docs, and skill references stay in sync.

## [2.50.0] - 2026-04-26

### Added

- **Per-workspace health metrics with `--group-by package`.** `fallow health --group-by package|owner|directory|section` finally produces a real grouped envelope instead of silently discarding the flag. JSON output keeps `vital_signs`, `health_score`, `summary`, `findings`, `file_scores`, `hotspots`, `large_functions`, and `targets` at the project level so consumers that ignore grouping still see the headline, then adds `grouped_by` plus a `groups` array where each bucket carries its own `vital_signs` and `health_score` recomputed from the group's files. SARIF results gain `properties.group` and CodeClimate issues gain a top-level `group` field, so GitHub Code Scanning and GitLab Code Quality can partition findings per team or package without dropping out of the SARIF/CodeClimate pipeline. Human output adds a per-group score / files / hot / p90 summary block sorted worst-first when `--score` is set, with a color-coded grade column and a `(root)` legend for files outside any workspace package. Compact, markdown, and badge fall back to ungrouped output with a stderr note pointing at `--format json`. The `check_health` MCP tool description now spells out the per-group output so AI agents can use it for per-team or per-package quality questions in a single invocation. Closes [#184](https://github.com/fallow-rs/fallow/issues/184).
- **VS Code extension: `fallow.changedSince` setting to scope LSP diagnostics to changed files.** New extension setting accepts a git ref (e.g. `main`, `HEAD~1`, `origin/main`); the LSP only emits diagnostics for files changed since that ref. Useful for legacy codebases adopting fallow incrementally without surfacing the entire pre-existing dead-code backlog at once. The status bar tooltip surfaces the active scope so users can see when diagnostics are filtered.

### Fixed

- **`fallow health --workspace X` now scopes `vital_signs`, `health_score`, and `summary.files_analyzed` to the workspace's files.** Previously the workspace flag scoped only `findings`, `file_scores`, and `hotspots`, leaving the project-level metrics at monorepo-wide values; consumers asking "what is the health score of workspace X" got the monorepo answer back. Now every per-module aggregate (cyclomatic distribution, total LOC, unit profiles), every `analysis_counts` denominator (dead files, dead exports, unused deps, circular deps), and the summary file count is recomputed against the workspace subset. Internal change: introduces `SubsetFilter` (`Full` / `Workspaces` / `Paths`) and `AnalysisCountsSnapshot` retained on `FileScoreOutput` so per-subset counts can be derived without re-running the analyser.
- **`fallow health --group-by package` on a non-monorepo emits exactly one structured JSON error instead of two.** The pre-existing flow validated `--group-by package` *after* the hotspot pipeline, so a non-monorepo run produced a `hotspot analysis requires a git repository` error followed by a `--group-by package requires a monorepo` error in the same stdout, breaking any pipeline that did `jq .` on the output. Validation now runs upfront so misconfigured invocations short-circuit before any expensive work and emit a single error object. The non-monorepo error message also suggests `--group-by directory` as a fallback for single-package projects.
- **VS Code status bar: `changedSince` value is HTML-escaped in the trusted-content tooltip and truncated to 40 characters in the status bar.** Avoids breaking tooltip rendering when the ref string contains markdown-active characters and prevents long refs (commit SHAs, full origin/branch names) from squeezing the rest of the bar.

## [2.49.0] - 2026-04-25

### Added

- **Per-analysis production mode for the programmatic API.** `analyze_project` now accepts an optional `production` configuration that overrides the global `FallowConfig.production` flag for that single call, so an embedder can run a check pass with `production: false` (developer-mode entry points, full graph) and a follow-up health pass with `production: true` (start/build entries only, type-only deps detected) against the same project state without rebuilding config. CLI users get the same toggle via the existing `--production` flag, but the new path is the embedder API surface (Vite plugin, GitHub Action library mode, IDE plugins). Programmatic callers see `production: bool` on the analysis options struct; CLI callers see no behavior change.

### Fixed

- **Cross-package enum and class members are no longer flagged as unused when consumed through a barrel re-export.** When `enum Foo { A, B, C }` is defined in `lib/types.ts`, re-exported by `lib/index.ts` (`export { Foo } from './types'`), and consumed by another workspace package via `import { Foo } from '@scope/lib'; Foo.A`, every member of `Foo` was reported unused. Phase 4 chain resolution synthesizes a stub `ExportSymbol` on barrel files for reference tracking, which is indistinguishable from a real local declaration by name alone, so the access map keyed at the barrel while the detection loop looked up accesses at the origin file (where `members` are populated). `find_unused_members` now walks each access key through `ReExportEdge` chains (named, renamed, and `export *` fan-out) to every defining-site export and copies the access set. Same fix applies to `whole_object_used_exports` for `Object.values(Foo)` patterns. Covers both `unused_enum_members` and `unused_class_members`. Real-world impact on vue-core: 17 false positives eliminated (12 enum + 5 class). Closes [#178](https://github.com/fallow-rs/fallow/issues/178).
- **Embedder API now respects per-analysis production config end-to-end.** `analyze_project` was reading the global `FallowConfig.production` flag for downstream filtering even when the per-call options overrode it, so the new per-analysis production mode silently fell back to the project default in the trace and reachability passes. The flatten step now threads the resolved per-analysis flag through every detector consistently.
- **GitLab CI template defaults FALLOW_PRODUCTION* variables to empty strings.** The `.fallow:base` template previously set `FALLOW_PRODUCTION_CHECK: "false"` and similar; pipelines that overrode these via `extends:` could not unset them because GitLab merges variable values rather than replacing. Switching the defaults to `""` makes the template idempotent under user overrides.
- **Health Istanbul coverage now keys by `(name, line, col)` instead of `(name, line)`.** Curried arrow functions that share a start line on the same column would merge their coverage records and double-count one branch. Adding the column disambiguates the rare collision without affecting any other coverage shape.

## [2.48.5] - 2026-04-25

### Added

- **MCP trace tools for chasing why a finding exists.** Four new tools land in the MCP server: `trace_export` (why is this export reachable, or why isn't it), `trace_file` (which entry points pull this file in), `trace_dependency` (which scripts and config files keep this package alive), and `trace_clone` (where else does this clone group appear). Each returns the full propagation chain, not just the verdict, so an agent can walk from "fallow says X" back to the entry point or script that justifies it without re-running analysis. `trace_dependency` correctly credits packages referenced only from `package.json` `scripts` blocks (e.g., husky, lint-staged) so they stop showing as unused in the trace; `trace_clone` strips absolute paths from instance locations so the output stays portable across machines, and validation errors share the same error envelope as the rest of the MCP surface. Closes [#176](https://github.com/fallow-rs/fallow/pull/176). Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza).

### Fixed

- **VS Code status bar tooltip now renders codicons instead of literal `$(error)` / `$(warning)` / `$(check)` text.** The popup markdown is built as a `vscode.MarkdownString` and the missing `supportThemeIcons = true` flag meant every codicon shorthand fell through as raw text. Independently, the status bar text and tooltip could disagree (e.g., `0.8% duplication` in the bar, `0.6% duplication` in the popup) because two code paths drove the surface: the LSP `analysisComplete` notification updated both, while CLI completion only updated the text. Both paths now feed through one `buildParamsFromCli` + `applyTooltipAndSeverity` pipeline so text and tooltip always derive from the same data. Crashed analysis runs now also surface as `setStatusBarError()` instead of a misleading "$(check) No issues found" tooltip. Closes [#179](https://github.com/fallow-rs/fallow/issues/179).
- **Health CRAP score no longer mismerges curried arrow functions that share a start line.** When two named arrow functions in the same file had identical line numbers (typical for one-line curried definitions like `const f = (a) => (b) => a + b`), the per-function CRAP merge collapsed them into a single record and double-counted coverage. The merge key now includes function name alongside `(file, line)` so curried arrows produce distinct CRAP entries.

## [2.48.4] - 2026-04-24

### Fixed

- **npm release pipeline recovery, second attempt.** v2.48.3 shipped on crates.io and GitHub Releases but the npm-publish job failed again: the v2.48.3 release commit had `crates/napi/package.json` at 2.48.3 while the lockfile's `packages.node_modules/@fallow-cli/fallow-node-*` entries pointed at 2.48.1, so CI's `npm ci --omit=optional` rejected the repo as out-of-sync. The working v2.48.1 pattern leaves `crates/napi/*` entirely unchanged in the release commit and lets CI's own "Update NAPI package version" step bump them at publish time. v2.48.4 reverts `crates/napi/package.json`, `crates/napi/package-lock.json`, and `crates/napi/index.js` to that in-sync 2.48.1 state; CI then bumps them from the tag after `npm ci` passes. No user-visible behavior change relative to v2.48.2 / v2.48.3; this ships the same PandaCSS plugin, Angular template fixes, and MCP rename in a form that finally makes it through the npm publish pipeline.

## [2.48.3] - 2026-04-24

### Fixed

- **npm release pipeline recovery.** v2.48.2 shipped on crates.io and GitHub Releases but the `npm-publish` job failed at the pre-publish `npm ci` in `crates/napi/` because the canonical sync step's `npm install --package-lock-only` dropped the `packages.node_modules/@fallow-cli/fallow-node-*` entries (npm could not resolve the new platform-package versions pre-publish). v2.48.3 restores the lockfile's nested package entries pointing at the last-published platform version (2.48.1), leaving `optionalDependencies` at 2.48.3; npm ci is happy because the platform packages are optional. No user-visible behavior change relative to v2.48.2. See [`48a5947d`](https://github.com/fallow-rs/fallow/commit/48a5947d) for the prior manifestation of the same drift.

## [2.48.2] - 2026-04-24

### Added

- **PandaCSS framework plugin.** `panda.config.{ts,js,mjs,cjs}` is no longer flagged as an unused file when `@pandacss/dev` is a dependency. The Panda CLI (`panda codegen`, `panda`) discovers the config by filesystem convention with no import edge, so static analysis alone cannot see it as used. The new built-in plugin mirrors the UnoCSS / Tailwind pattern: enabler `@pandacss/dev`, config patterns and always-used on `panda.config.{ts,js,mjs,cjs}`, tooling dependencies for `@pandacss/dev` / `@pandacss/studio` / `@pandacss/eslint-plugin`, and `resolve_config` that captures both ES imports and shallow-string `presets` entries so preset packages like `@pandacss/preset-panda` and `@park-ui/panda-preset` are not flagged as unused. Closes [#175](https://github.com/fallow-rs/fallow/issues/175).

### Changed

- **MCP tool `health_production_coverage` renamed to `check_production_coverage`** for picker parallelism with `check_health` and `check_changed`. The tool description now leads with `(paid)`, and the server instructions gain a disambiguation line so agents pick the right tool between `check_health` (static, free) and `check_production_coverage` (V8-runtime-backed, paid). `check_health` also gains `min_observation_volume` and `low_traffic_threshold` params to reach parity with the underlying CLI flags, and `check_production_coverage` gains `group_by` for CODEOWNERS / directory / package / section grouping. Breaking change on the MCP wire protocol (tool name); accepted since this is a paid preview tool with no production users yet.

### Fixed

- **Angular inherited and DI-injected members referenced in external templates are no longer reported as unused.** Two independent root causes: (1) child-component template references bridged into `self_accessed_members` were not propagated up the `extends` chain to the base class's file (e.g., a base component's lifecycle hooks or helpers used in a child's `templateUrl`), and (2) the template scanner captured only top-level identifiers (losing `getTotal` in `{{ dataService.getTotal() }}`), and constructor-param types from DI were not exposed on `ModuleInfo`, so standalone-parsed HTML could not resolve chains to injected services. Scanner now returns member-access chains alongside identifiers; constructor params with accessibility modifiers and typed property declarations are captured into `ClassHeritageInfo.instance_bindings`; the analysis bridge resolves HTML chains through the importing component's bindings. Cache version bumped to 47. Closes [#174](https://github.com/fallow-rs/fallow/issues/174).

## [2.48.1] - 2026-04-24

### Fixed

- **`fallow setup-hooks` now ships a gate script that refuses to run with a stale fallow on `PATH`.** A fallow older than v2.46.0 (before the uncommitted-changes inclusion fix, commit `aabb8e1b`) silently passed audits that newer fallow would fail, defeating the purpose of the Claude Code `git commit` / `git push` gate. The generated `.claude/hooks/fallow-gate.sh` now enforces a `FALLOW_GATE_MIN_VERSION` floor (default `2.46.0`) and blocks with a copy-pasteable upgrade hint when the binary resolved from `PATH` is below it. Override with `FALLOW_GATE_MIN_VERSION=<semver>` or disable with an empty string. Every verdict=fail block now also prepends `fallow-gate: blocked by fallow <version> at <binary>` to stderr so the responsible binary is identifiable without re-diagnosis, and the installed script stamps `# Installer version: <semver>` in its header for forensics (substituted at install time from `fallow setup-hooks`'s own version). See <https://docs.fallow.tools/integrations/claude-hooks#version-floor>.

### Changed

- **Internal unit-size refactors with no behavioral change.** `find_unused_members` class inheritance propagation split into `build_parent_to_children` + `propagate_class_inheritance` helpers; `build_human_lines` / `build_sarif` / `build_codeclimate` each split into per-section helpers; package-path dep sections in CodeClimate collapsed behind a `NamedPkgDep` trait. Output byte-identical across all formats; same fingerprints, same severities, same issue ordering.

## [2.48.0] - 2026-04-23

### Added

- **Agent skill now ships inside the `fallow` npm package.** The `skills/fallow/` content (SKILL.md plus CLI reference, gotchas, and patterns references) is now packaged alongside the CLI wrapper, so `npm install fallow` places the skill on disk next to the binary the agent drives. This removes version drift between the installed binary and the skill an agent loads from a separately-cloned `fallow-skills` checkout, and removes the manual clone step from the onboarding path. Claude Code, Codex, and other agents that discover skills on disk pick it up without any extra configuration. The same content remains published from the `fallow-skills` repo for users who prefer plugin-style installs. A CI lane now verifies the bundled skill stays in sync with the source of truth on every PR. Closes [#173](https://github.com/fallow-rs/fallow/issues/173). Thanks [@OmerGronich](https://github.com/OmerGronich) for the proposal.

## [2.47.1] - 2026-04-23

### Fixed

- **Windows ARM64 (`aarch64-pc-windows-msvc`) is now a first-class release target.** `npx fallow` previously hard-failed on Windows ARM devices because no native binary was published. The release pipeline now builds and publishes `@fallow-cli/win32-arm64-msvc` for the CLI / LSP / MCP binaries and `@fallow-cli/fallow-node-win32-arm64-msvc` for the Node bindings, and the npm wrapper plus VS Code extension downloader both resolve the new target. A PR-time CI lane on native Windows ARM64 runners compiles the CLI and the NAPI addon so the target is covered before release instead of only at tag time. The npm platform resolution was also factored into a shared helper with tests so future targets stay consistent. Closes [#165](https://github.com/fallow-rs/fallow/issues/165). Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza) ([#171](https://github.com/fallow-rs/fallow/pull/171)).

## [2.47.0] - 2026-04-23

### Added

- **Native Node.js bindings package (`@fallow-cli/fallow-node`).** New async NAPI-RS bindings expose the main one-shot analyses directly inside Node without spawning the CLI: `detectDeadCode`, `detectCircularDependencies`, `detectBoundaryViolations`, `detectDuplication`, `computeComplexity`, and `computeHealth`. The bindings reuse the CLI orchestration layer and return the same JSON report envelopes the CLI emits, including `schema_version`, `summary`, relative paths, and injected `actions`. The Rust-side programmatic facade lives in `fallow-cli::programmatic`, and the repo now includes a temp-project Node smoke test plus CI/release wiring for the addon package. The JS API accepts lowercase CLI-style enum literals (`"mild"`, `"cyclomatic"`, `"low"`, `"handle"`) and rejected promises now expose structured fallow fields like `exitCode`, `help`, and `context`. See <https://docs.fallow.tools/integrations/node-bindings>.

### Fixed

- **Benchmark JSON parsing hardened against partial runs.** The benchmark harness used a best-effort parser that silently treated truncated or malformed JSON as "zero findings", so a crashed run scored as a clean run. Parsing now validates the envelope and rejects invalid payloads up front, so partial results no longer poison cross-tool comparisons.
- **Benchmark validity checks tightened; Vite overhead reduced.** The benchmark validity gate used to accept runs whose `summary.total_issues` silently disagreed with the sum of its per-category counts. It now rejects those runs, matching the stricter behavior the public comparison tables rely on. Adjacent Vite bench setup no longer installs dev dependencies it never invokes, cutting per-run overhead.
- **Recursive plugin config scans avoided.** Some framework plugins re-scanned the project config object every time a file was processed, producing redundant work on large monorepos. The scans now memoize the resolved config per analysis run, removing the redundant traversals.
- **`unused-listed-deps` no longer false-positives on broken `tsconfig.json` path aliases.** A `tsconfig.json` whose `compilerOptions.paths` pointed at a non-existent prefix would cause the resolver to fall through to "unlisted" on imports that were actually listed in `package.json`, flagging legitimate dependencies as unused. The resolver now treats unresolved alias prefixes as transient rather than definitive, so the listed-dep check only fires on genuinely missing entries.

### Changed

- **`cargo shear` no longer flags `tokio` in `crates/napi`.** The Node bindings crate no longer declares a direct `tokio` dependency; the napi `tokio_rt` feature already pulls it transitively.
- **NAPI option parsing no longer silently truncates `maxCyclomatic` / `maxCognitive`.** Both fields are now validated against `u16::MAX`; out-of-range values produce a typed `InvalidArg` napi error instead of wrapping to a different threshold.
- **`sync-npm-versions.sh` now rewrites the version strings NAPI-RS bakes into `crates/napi/index.js`.** Bumping a release used to leave the hardcoded `!== 'X.Y.Z'` comparisons at the previous tag, so strict `NAPI_RS_ENFORCE_VERSION_CHECK` consumers saw spurious mismatches on every release. The sync script now rewrites those literals in lockstep with `package.json`.

## [2.46.0] - 2026-04-23

### Added

- **`fallow setup-hooks` generates a Claude Code `PreToolUse` gate** that intercepts Claude's `git commit` / `git push` Bash tool calls, runs `fallow audit --format json --quiet --explain`, and blocks with exit 2 + JSON-to-stderr on `verdict: "fail"`. The agent reads the full audit envelope (including `_meta.docs` links and `actions` arrays on every finding), fixes the code, and retries. Runtime errors fail open with a single stderr notice so skips stay visible rather than silently trusting a missing gate. Writes project-level `.claude/settings.json` + `.claude/hooks/fallow-gate.sh` (chmod 755 on Unix), and optionally maintains an idempotent managed block in `AGENTS.md` between `<!-- fallow:setup-hooks:start/end -->` markers as a Codex fallback while Codex hooks stabilise. Flags: `--agent claude|codex`, `--dry-run`, `--force`, `--user` (installs globally under `~/.claude/` with a `$HOME`-based handler command), `--gitignore-claude`, `--uninstall`. The uninstall path removes the fallow handler while preserving any non-fallow handlers in the same matcher group, deletes the hook script if it still carries the generator marker, and strips the managed AGENTS.md block; idempotent on repeat runs. Re-running the installer upgrades handlers written by earlier manual setups (absolute paths, `~/`-based paths, Windows drive paths) to the canonical `"$CLAUDE_PROJECT_DIR"` form without leaving duplicates. See <https://docs.fallow.tools/integrations/claude-hooks>.
- **Zed editor extension.** Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza) ([#164](https://github.com/fallow-rs/fallow/pull/164)).

### Fixed

- **Health baselines survive line-number churn.** The health baseline keyed on `file + function + line_start`, so any refactor that shifted lines invalidated every entry and reported the same findings as "new." The key now uses `file + function + qualified_name` (line-stable), mirroring the dead-code and duplication baselines. Closes [#169](https://github.com/fallow-rs/fallow/issues/169). Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza).
- **`fallow --save-baseline <path>` auto-creates the parent directory.** Copy-pasting the `fallow-baselines/dead-code.json` example into a fresh repo used to error with "No such file or directory" because the baseline writer never ran `mkdir -p`. Applies to `dead-code`, `health`, and `dupes` save-baseline paths.
- **Shallow nested package scripts are discovered again.** The package-script walker stopped descending into `workspaces/*/package.json` entries that lived one directory below the root, silently dropping nested script declarations from the entry-point graph. The walker now follows shallow nesting the same way it follows deep nesting.

## [2.45.1] - 2026-04-22

### Fixed

- **Istanbul `--coverage` now matches functions produced by standard Istanbul tooling (Jest, nyc, c8, babel-plugin-istanbul).** Standard Istanbul producers omit the non-standard `FnEntry.line` field that `oxc-coverage-instrument` writes, so fallow's `load_istanbul_coverage` silently defaulted the line to `0` and every lookup failed — `istanbul_matched: 0` out of N, all CRAP scores fell back to binary estimation, and `--max-crap` ran against estimates instead of real coverage. The loader now falls back to `FnEntry.decl.start.line` when `FnEntry.line` is missing, restoring the anonymous-by-line and fuzzy lookup paths for Jest/nyc/c8-produced coverage while preserving the fast path for `oxc-coverage-instrument` output. Closes [#166](https://github.com/fallow-rs/fallow/issues/166).
- **Repeated "entry point outside project root" warnings collapse into a single diagnostic.** Monorepos with shared entry points (e.g. workspace `package.json` pointing at a sibling build output) used to emit the same warning once per analysis target, spamming `stderr` with dozens of identical lines. The warnings are now deduplicated with a counted summary so the signal stays visible without drowning out other diagnostics.
- **`fallow dupes` tuning flags (`--min-lines`, `--min-tokens`, `--mode`, `--cross-language`) are forwarded correctly in combined GitLab CI mode.** The wrapper's combined-mode `ARGS=()` block built the `dupes` invocation without reading the `FALLOW_DUPES_*` env vars, so user overrides set in `.gitlab-ci.yml` silently became defaults. Fix pairs with the same GitHub Action fix from v2.44.x so both CI integrations honor the same tuning surface.
- **Entry point discovery and VS Code extension downloads are hardened.** The entry-point walker now tolerates package manifests with missing / non-object `bin` or `exports` fields instead of aborting discovery, and the VS Code extension's binary-download path validates content-length + checksum against the expected artifact before unpacking (no more silent half-downloads that fail the first `fallow` invocation with an opaque error).
- **CLI `stderr` noise reduced on normal runs.** Incidental `tracing` / `println!` lines that leaked into `stderr` during analysis (progress prints, one-off debug lines) are gone; `FALLOW_QUIET=1` runs are byte-clean on stdout/stderr for scripting integrations.
- **`--changed-since` now includes untracked files that were added in the working tree but not yet committed.** Changed-scope filtering based on `git diff` missed brand-new files until they were staged, so a PR-style run (`--changed-since origin/main`) could falsely report zero issues on new code. Untracked-and-added paths are now folded into the changed set.
- **Production coverage signal alignment in CI reporting.** `fallow health --production-coverage` output now consistently reports the same matched / unmatched / stale counts across human, JSON, markdown, and the GitHub Action / GitLab CI summary + annotation pipelines. Prior releases could show different subtotals depending on whether combined mode or the standalone command ran.

## [2.45.0] - 2026-04-22

### Added

- **Static inventory extractor (`fallow-extract`).** New extraction phase that produces a per-file list of exported functions keyed by `(path, name, line)`. Feeds the line-aware production-coverage matcher so runtime samples can be joined against the AST view of "every function that exists" without collisions on overloaded names.
- **`fallow coverage upload-inventory`.** Uploads the static function inventory to fallow cloud so the Production Coverage dashboard can filter `untracked` functions (the AST side of the join: functions that exist in the code but were never observed at runtime). Requires a valid license. See <https://docs.fallow.tools/analysis/production-coverage>.

### Fixed

- **External CSS packages imported from Vue SFC `<style>` blocks are credited as used.** CSS-only design-system packages imported only through `<style src="pkg/dist/style.css">` or an `@import "pkg/..."` inside a Vue SFC `<style>` block used to be reported as `unused-dependencies` because the Vue plugin only walked script imports. A new external-style scanner now recognizes style-side package imports declared in `.vue` SFCs and credits the npm package, eliminating the false positive for Vue + Vite design-system codebases.
- **`compilerOptions.paths` aliases in `tsconfig.json` are honored.** Path aliases like `"@/*": ["src/*"]` declared in `compilerOptions.paths` are now extracted by the TypeScript plugin and registered as resolver path aliases, so `@/foo` imports resolve to `src/foo` in projects that rely on alias-only paths without a matching `vite.config` / `webpack.config` rewrite.
- **`tsconfig.references` entries pointing at a `.json` file directly are followed.** References of the form `{ "path": "./tsconfig.app.json" }` are now resolved to the literal file rather than getting `tsconfig.json` concatenated onto them, fixing reference-chain walking for the common "one tsconfig per build" pattern.
- **Coverage inventory upload aligned with line-aware function identity.** `fallow coverage upload-inventory` now emits records keyed by `(file, name, line)` to match the line-aware sidecar protocol introduced alongside v2.44.x, replacing the older line-less identity that could collide on overloaded / re-declared names. Upload integration hardened against CI-only path normalization regressions that surfaced on Linux runners.

## [2.44.2] - 2026-04-21

### Fixed

- **Class members used only through interface-typed bindings are no longer flagged unused.** When a class method was called only via an interface-typed variable or parameter (e.g. `const strategy: VirtualScrollStrategy = ...; strategy.attach();` where `FixedSizeScrollStrategy implements VirtualScrollStrategy`), the method appeared as an unused class member because the access resolved to the interface name, not the implementer. The extractor now tracks type-annotated bindings (locals, parameters, class fields, parameter properties) alongside `new ClassName()` bindings, and the member-usage analysis propagates interface member accesses to every class that `implements` the interface. Same-named interface exports in separate files stay isolated by `ExportKey`, so unrelated implementers do not silently credit each other. Cache version bumped to 44 so warm caches pick up the new extraction records on upgrade. Closes [#132](https://github.com/fallow-rs/fallow/issues/132). Thanks [@M-Hassan-Raza](https://github.com/M-Hassan-Raza).
- **`react-router.config.ts` route modules with a `routesFn` are honored.** Config-driven route modules (`import { flatRoutes } from "@react-router/fs-routes"; export default { routes: flatRoutes() }`) are now extracted the same way as static `routes: [...]` arrays, so route files referenced only through the filesystem convention are no longer reported as unused files in React Router 7 apps.
- **`rand 0.8` re-pinned in `fallow-license` dev-dependencies.** A Dependabot bump to `rand 0.9` broke the `OsRng` + `SignatureEncoding` wiring used by test key generation; the release build never exercised that path so CI missed the regression. Dev-deps now stay on 0.8 until the signing path is ported to 0.9's new `TryRngCore` trait set.

## [2.44.1] - 2026-04-21

### Fixed

- **Istanbul `--coverage` now matches arrow-function exports against `(anonymous_N)` entries.** Istanbul records arrow functions bound to a `const` (e.g. `export const handler = () => {...}`) as `(anonymous_0)`, while fallow extracts the binding identifier (`handler`). Neither the exact `(name, line)` match nor the name-only fuzzy match succeeded, so arrow-heavy codebases silently fell through to the estimated coverage model with `istanbul_matched: 0`, and `--max-crap` ran against estimates instead of real coverage. `IstanbulFileCoverage::lookup` now has a third fallback: when no name-based match exists and exactly one `(anonymous_N)` entry starts within ±2 lines of the requested line, use it. The single-candidate guard keeps the match unambiguous (multiple candidates return `None` rather than risk misattribution). Closes [#155](https://github.com/fallow-rs/fallow/issues/155).

## [2.44.0] - 2026-04-21

### Added

- **`--max-crap <N>`: per-function CRAP threshold on `fallow health` and `fallow audit`.** New threshold flag (and `health.maxCrap` config key, default 30.0) that fails the command when any function's CRAP (Change Risk Anti-Patterns) score meets or exceeds the threshold. Mirrors the existing `--max-cyclomatic` / `--max-cognitive` gates but uses the Savoia and Evans (2007) canonical formula `CC^2 * (1 - cov/100)^3 + CC`, which combines complexity with coverage so an AI-assisted workflow can force refactors or test coverage on exactly the functions that combine both risk dimensions. Pair with `--coverage ./coverage/coverage-final.json` for Istanbul-backed per-function coverage; without Istanbul data fallow estimates coverage from the module graph (directly test-referenced = 85%, indirectly reachable = 40%, untested = 0%). `HealthFinding` gains optional `crap` and `coverage_pct` fields and the `exceeded` enum gains four CRAP-containing variants (`crap`, `cyclomatic_crap`, `cognitive_crap`, `all`); the CRAP-only finding carries an `add-tests` action in JSON output because coverage is the leverage point, not refactoring. Findings are filterable by `--baseline` / `--save-baseline` the same way complexity findings are. SARIF adds a new `fallow/high-crap-score` rule (12 health rules total), CodeClimate maps CRAP findings to a matching check name, markdown renders a dedicated CRAP column, and human output appends a third metrics line with `(N% tested)` when Istanbul matched. MCP surfaces `max_crap` on `check_health`, `audit`, and `health_production_coverage`. GitHub Action and GitLab CI expose the input with floating-point validation and thread it through the jq summary, annotation, and review-comment templates. Closes [#141](https://github.com/fallow-rs/fallow/issues/141).
- **`development` export condition honored by the resolver.** The Oxc resolver is now configured with `"development"` in the baseline condition set alongside `import`, `require`, `default`, `types`, and `node`. Packages that publish a `development` branch in `exports` / `imports` (common pattern: source files for dev, compiled output for prod; used by Vite, Vitest, esbuild, Rollup) now resolve deterministically to source when fallow walks them. The `resolve.conditions` config key lets users prepend custom condition names (e.g. `["test"]`, `["bun"]`) ahead of the baseline so codebases with custom conditional-export setups can opt into the variants that match their toolchain. When the React Native or Expo plugin is active, `"react-native"` and `"browser"` prepend automatically as before. Closes [#135](https://github.com/fallow-rs/fallow/issues/135).

### Changed

- **README summary block matches actual `--summary` output.** The hero block on the README used to show a hand-written sketch of what `fallow --summary` produced; it now mirrors the real output byte-for-byte so new users scanning the front page see the same column alignment and labels they will encounter when they run the command.

## [2.43.0] - 2026-04-21

### Added

- **`fallow audit` per-analysis baselines.** Three new flags on the `audit` command let each sub-analysis (dead code, health, duplication) filter against its own saved baseline: `--dead-code-baseline <PATH>`, `--health-baseline <PATH>`, `--dupes-baseline <PATH>`. Baselines reuse the formats produced by `fallow dead-code --save-baseline`, `fallow health --save-baseline`, and `fallow dupes --save-baseline` respectively (no new format). Issues present in the respective baseline are excluded from the audit verdict, so touching a legacy file with pre-existing findings no longer makes `fallow audit` fail on a PR that introduces no new issues. Defaults can be set in `.fallowrc.json` under `audit.deadCodeBaseline` / `audit.healthBaseline` / `audit.dupesBaseline`, with CLI flags overriding. Relative paths from either source resolve against `--root` so CI scripts using `--root $REPO_ROOT` behave consistently with interactive use. The MCP `audit` tool exposes the same three params (`dead_code_baseline`, `health_baseline`, `dupes_baseline`). Closes [#139](https://github.com/fallow-rs/fallow/issues/139).
- **`group_by` on `find_dupes`, `check_health`, and `audit` MCP tools.** The three sibling tools now accept the same `group_by` param (`"owner"` / `"directory"` / `"package"` / `"section"`) that `analyze` already accepts, so agents can partition duplication, complexity, and audit output the same way they partition dead-code output. Fixes an oversight from the `--group-by section` shipping in v2.42.0 where the flag reached every CLI surface but only one of four MCP tools.

### Changed

- **Global `--baseline` / `--save-baseline` are now rejected on `fallow audit`** (exit 2) with an error pointing users at `--dead-code-baseline`, `--health-baseline`, and `--dupes-baseline`. Audit runs three sub-analyses with different baseline formats, so a single baseline path was ambiguous. Previously the global flag was silently ignored on `audit`. If you were passing it on audit before, it was already a no-op; switch to the per-analysis flags.

### Fixed

- **Nuxt shared auto-imports are no longer flagged as unused.** Composables and utilities exported by auto-import-aware Nuxt packages (`#imports`, framework-scoped auto-imports) were surfacing as unused-export false positives in projects that relied on the auto-import dance instead of explicit `import` statements. The Nuxt plugin now recognizes these shared auto-imports and treats them as always-reachable.

## [2.42.0] - 2026-04-20

### Added

- **`--group-by section`: GitLab CODEOWNERS section grouping.** New `--group-by` mode that partitions output by GitLab CODEOWNERS `[Section]` header name instead of by primary owner. Distinct sections stay in distinct groups even when they share a default reviewer, so repos where one common approval group is listed first on every section no longer collapse every finding into a single bucket. In JSON output each group carries an `owners: string[]` array populated from the section's default owners. Rules declared before any section header fall into the `(no section)` bucket; unmatched paths continue to use `(unowned)`. Human and markdown formats surface the owners under each group header. `--group-by section` against a CODEOWNERS file that contains no `[Section]` headers errors out with a pointer to `--group-by owner`, rather than silently falling back. MCP's `analyze` tool forwards `section` as a valid `group_by` value with `owners: string[]` metadata documented on the param. Closes [#133](https://github.com/fallow-rs/fallow/issues/133).
- **`health_production_coverage` MCP tool: focused paid production-coverage entry point.** New MCP tool that wraps `fallow health --production-coverage <path>` with a narrow parameter surface so agents can discover the right tool by name instead of passing special fields to the general `check_health` tool. Required `coverage` param (V8 directory, V8 JSON, or Istanbul JSON file). Optional scope passthroughs: `root`, `config`, `production`, `workspace`, `no_cache`, `threads`. Tuning knobs: `min_invocations_hot`, `min_observation_volume`, `low_traffic_threshold`. Tool description warns about raising `FALLOW_TIMEOUT_SECS` for multi-megabyte dumps that exceed the 120s default MCP subprocess timeout.
- **Production-coverage `capture_quality` summary and trial CTA.** Bumps `fallow-cov-protocol` from 0.2 to 0.3 and wires the new `Summary.capture_quality` block (`window_seconds`, `instances_observed`, `lazy_parse_warning`, `untracked_ratio_percent`) through the health pipeline. When the sidecar flags a short-window or lazy-parse capture, the human renderer shows a yellow `note:` caveat plus a quantified trial CTA for `fallow license activate --trial`; long / clean captures stay quiet. Markdown surfaces the warning for PR comments (no CTA). JSON passes the block through for agent consumers. `schema_version` stays at 4 per the additive-fields policy.
- **`--changed-workspaces <REF>`: git-derived monorepo CI scoping.** New global flag that auto-detects which workspaces contain any file changed since the given git ref (e.g. `origin/main`, `HEAD~1`) and scopes output to that set. Removes the need for CI jobs to hand-maintain a `--workspace a,b,c` list as the package graph evolves. Reuses the existing `--workspace` filter path end-to-end, so dead-code, health (complexity + hotspots + coverage), flags, audit, and the combined pipeline all respect the derived scope identically. Mutually exclusive with `--workspace`: validated at the CLI layer with a targeted error. Unlike `--changed-since`, a missing ref or non-git directory is a hard error (exit 2) rather than a silent full-scope fallback. The flag's entire purpose is to narrow CI scope, so quietly widening back to the whole monorepo would defeat the optimization. When git succeeds but no tracked workspace file changed (e.g. a root-only lockfile bump), the scope is empty and the pipeline exits 0 with no issues reported. GitHub Action and GitLab CI both expose the new input and wire it through the analyze pipeline. Closes knip PR #1455 parity gap.

### Fixed

- **`dupes` now respects `--workspace` and `--changed-workspaces` scoping.** Previously `fallow dupes --workspace X` returned every clone group in the repo, as did combined mode (`fallow --workspace X` / `fallow --changed-workspaces REF`). The workspace flag was wired into `check` and `health` but not `dupes`, so in a monorepo every duplication report spanned the whole project regardless of scope. Surfaced during the post-ship end-to-end test of `--changed-workspaces` on next.js, where combined mode reported 4780 clone groups despite a narrow scope. Fixed by adding `workspace` / `changed_workspaces` to `DupesOptions`, a new `filter_by_workspaces` pass that retains clone groups with at least one instance under any selected workspace root (rebuilding clone families, mirrored-directory detection, and duplication stats from the survivors), and wiring it through the standalone `fallow dupes`, combined, and `fallow audit` entry points. No schema changes; reported `duplication_percentage` now reflects the scoped slice instead of the whole repo.
- **Nuxt CSS aliases resolve through `normalize_nuxt_path`.** Local CSS entries (`~/`, `~~/`, `@/`, `@@/`, `./`, `/`) now route through the same alias resolver as plugins, components, and aliases, so they match how the rest of the Nuxt plugin already resolves paths. Bare specifiers stay treated as npm package CSS. Adds an integration fixture (`nuxt-css-alias`) plus unit coverage for nested-config and `srcDir: 'app'` combinations.
- **`FALLOW_LICENSE_PATH=""` is now treated as unset.** A shell exporting an empty-string `FALLOW_LICENSE_PATH` previously produced a cryptic `license I/O error: No such file or directory (os error 2)` on any paid-feature command, because `std::env::var` returns `Ok("")` and `load_raw_jwt` handed that to `Path::new("")`. Empty and whitespace-only values now fall through to default discovery, matching the user mental model and mirroring the neighboring `FALLOW_LICENSE` normalize-and-check-empty pattern.

## [2.41.0] - 2026-04-20

### Added

- **`--workspace` accepts multiple patterns with globs and `!` negation.** The global `-w` / `--workspace` flag is no longer limited to a single exact package name; it now accepts comma-separated values (`-w web,admin`), repeated flags (`-w web -w admin`), glob patterns matched against BOTH the `package.json` name AND the workspace path relative to the repo root (`-w 'apps/*'`, `-w '@scope/*'`), and `!`-prefixed negations (`-w '!apps/legacy'`). Combinations follow gitignore-style rules: positive-only includes matches, negative-only starts from all workspaces and removes matches, mixed applies positives then subtracts negatives. Literal package names that happen to contain glob metacharacters (e.g. `web-[staging]`) are handled by an exact-name short-circuit so they still match without any quoting dance. Single-value usage (`-w my-package`) stays fully back-compatible. Propagated through `dead-code`, `health` (including hotspots, coverage sidecar, large-function analysis), `flags`, `audit`, and the combined pipeline. MCP `workspace` param keeps its `Option<String>` shape so existing agent integrations continue to work; docstrings document the new syntax. Unmatched positive patterns are collected into a single "no workspaces matched patterns: 'X', 'Y'. Available: a, b, ..." error (exit 2) rather than one error per miss, and the available-workspaces list caps at 10 names for large monorepos. The "all workspaces excluded" error splits Included vs Excluded so a typo in a negation pattern is immediately visible. Closes knip issue #1441.

### Fixed

- **Sidecar discovery now prefers the real platform binary over the Node wrapper.** After `npm install @fallow-cli/fallow-cov`, `node_modules/.bin/fallow-cov` is a Node wrapper script that re-execs the real binary in the platform subpackage (e.g. `@fallow-cli/fallow-cov-darwin-arm64/fallow-cov`). The wrapper path has no adjacent `.sig` file, so the Phase 2.5 signature verification gate refused to spawn with `Sidecar binary at ... is missing its signature file` (exit 4) even though the signed binary and its `.sig` were sitting right next to each other in the platform subdirectory. Discovery now walks up ancestors looking for `node_modules/@fallow-cli/fallow-cov-*/fallow-cov` first, then falls back to the `.bin` wrapper for non-npm-optionalDependency layouts. Every v2.40.0 / v2.40.1 / v2.40.2 user running `fallow health --production-coverage` for the first time hit this; `FALLOW_COV_BINARY_PATH=...` worked around it but the happy path should not require an env var. Found via Phase 2.5 end-to-end smoke test against sidecar-v0.1.5.

## [2.40.2] - 2026-04-17

### Fixed

- **`super.method()` calls now credit the parent class's method as used.** Base-class methods only invoked via `super.method()` from subclasses were silently dropped by the AST visitor and surfaced as false-positive `unused-class-member` findings. A `class_super_stack` now tracks the nearest enclosing class's `extends` identifier while walking the class body; `super.<member>` expressions emit a member access against that identifier, and the existing `local_to_imported` resolver in `find_unused_members` maps it back to the parent's export name. Object-literal `super` (no enclosing class on the stack) is correctly dropped, and nested class expressions respect their own frame. Verified against svelte (13 false positives eliminated, each traced to a real `super.<member>` call in source) and vite (-2 false positives); zero new findings introduced on any benchmark fixture. Closes [#130](https://github.com/fallow-rs/fallow/issues/130).

## [2.40.1] - 2026-04-17

### Fixed

- **`BINARY_SIGNING_VERIFY_KEY` rotated to match the key pair now deployed in the sidecar release pipeline.** The first two `fallow-rs/fallow-cloud` sidecar release tags (`sidecar-v0.1.0`, `sidecar-v0.1.1`) failed in CI because the `ED25519_BINARY_SIGNING_PRIVATE_KEY` GitHub secret had been set with a malformed `gh secret set --body -` invocation that stored the literal `-` rather than reading the seed from stdin. Recovering the original seed was not possible (write-only secrets, local env scrubbed after write), so a fresh key pair was provisioned across Fly (staged), GitHub Actions secret (validated via `workflow_dispatch`), and GitHub Actions variable. This commit updates the compiled-in verify key so v2.40.1+ CLI binaries accept signatures produced by the new signing key. v2.40.0 remains safe -- no sidecar binaries were ever published under the broken configuration (`sidecar-v0.1.0` / `sidecar-v0.1.1` reached neither npm nor the GitHub release assets), so no user-visible regression to undo. First real signed sidecar will ship against v2.40.1 + `sidecar-v0.1.2`.

## [2.40.0] - 2026-04-17

### Added

- **Scoped `usedClassMembers` rules** -- `usedClassMembers` entries may now be either a plain member name (global suppression, as before) OR a scoped object with `extends` / `implements` constraints. Framework-invoked method names that are too common to globally suppress (e.g. TypeORM `MigrationInterface.up` / `down`, ag-Grid `agInit`, Web Components `connectedCallback`) can be scoped so they only suppress on classes matching the specified heritage clause. Plain string entries keep their previous semantics; schema is a backward-compatible superset. New `UsedClassMemberRule` / `ScopedUsedClassMemberRule` types exposed in `schema.json` and `plugin-schema.json`. Thanks @M-Hassan-Raza ([#129](https://github.com/fallow-rs/fallow/pull/129)).
- **`FALLOW_COV_BINARY_PATH` env var** -- second explicit-override slot for the production-coverage sidecar binary, ordered between `FALLOW_COV_BIN` and project-local lookup. Required for air-gapped enterprise installs, Linux distro-packaged sidecars, and Docker multi-user setups where `~/.fallow/bin` is not writable. Missing-file returns a clear error instead of silently falling through to auto-discovery, matching `FALLOW_COV_BIN`'s explicit-beats-implicit semantics.
- **Ed25519 signature verification on every sidecar spawn** -- the `fallow-cov` production-coverage sidecar binary must ship with an adjacent `<binary>.sig` file. `fallow health --production-coverage` verifies the signature against a compiled-in public key (`BINARY_SIGNING_VERIFY_KEY`) before executing the sidecar and fails hard (exit code 4) on any of: missing `.sig` file, wrong signature length, or invalid signature. Strict from day one -- no warn-and-run, no opt-out env var. Hardens the sidecar supply chain across npm mirrors, air-gapped installs, and ad-hoc binary placement. Error messages direct users to `npm install @fallow-cli/fallow-cov`.

### Fixed

- **`fallow license` non-Unix stub** no longer fails clippy under rust 1.95 -- the `#[cfg(not(unix))]` permission-set stub returns `Result<(), String>` to mirror the Unix variant's signature, which rust 1.95's `unnecessary_wraps` flags. Added an explicit `#[allow]` with a reason.

## [2.39.0] - 2026-04-17

### Added

- **Production Coverage Intelligence (Phase 2)** -- `fallow health --production-coverage <path>` merges V8 or Istanbul runtime coverage into the existing health report. Spawns the closed-source `fallow-cov` sidecar (distributed separately; binary distribution deferred to Phase 2.5) and surfaces a typed report with stable content-hash IDs, per-finding verdicts, a supporting evidence block, hot-path percentiles, and a `license-expired-grace` watermark track. Paid feature — gated on a valid Ed25519-signed license JWT with the `production_coverage` feature.
- **`fallow license` subcommand** -- `activate` (with `--trial --email <addr>` for zero-credit-card onboarding or `--from-file <path>` / stdin for existing licenses), `status`, `refresh`, `deactivate`. JWT verification is fully offline against a compiled-in Ed25519 public key; only `activate --trial` and `refresh` make network calls (5s connect timeout, 10s total). License is stored at `~/.fallow/license.jwt` (or `$FALLOW_LICENSE_PATH`) with owner-only permissions on Unix; the Windows fallback honours `%USERPROFILE%`. Grace ladder matches Docker Desktop / JetBrains: 0–7 day warning, 7–30 day watermark, 30+ day hard-fail.
- **`fallow coverage setup`** -- single-entry-point resumable first-run flow that walks a user through: license check, `fallow-cov` sidecar discovery/install, framework-specific coverage recipe (Next.js / Nest / SvelteKit / Remix / Astro / plain Node), and automatic handoff into `fallow health --production-coverage`. Sidecar discovery resolves `$FALLOW_COV_BIN` → `./node_modules/.bin/fallow-cov` → package-manager bin → `~/.fallow/bin/fallow-cov` → `$PATH`.
- **`--min-observation-volume <N>`** on `fallow health` -- minimum total trace volume before the sidecar is allowed to emit high-confidence `safe_to_delete` / `review_required` verdicts. Below this threshold the sidecar caps confidence at `medium`. Defaults to the spec value (5000) when omitted.
- **`--low-traffic-threshold <RATIO>`** on `fallow health` -- fraction of total trace count below which an invoked function is classified as `low_traffic` rather than `active` (e.g. `0.001` for 0.1%). Defaults to the spec value when omitted.
- **`fallow-cov-protocol 0.2.0`** -- new published crate (`crates.io`) holding the versioned envelope types shared between the public CLI and the closed-source sidecar. Stable content-hash IDs (`fallow:prod:<hash>` / `fallow:hot:<hash>`), per-finding `Verdict` / `Confidence` / `Evidence`, `ReportVerdict` renaming, `StaticFunction::static_used` + `test_covered` as required fields, new `Options` knobs, and forward-compat `Unknown` sentinels on every enum.
- **SARIF rules for production coverage** -- `fallow/production-safe-to-delete`, `fallow/production-review-required`, `fallow/production-low-traffic`, `fallow/production-coverage-unavailable`, `fallow/production-coverage`. Each rule has `fullDescription` and `helpUri` from the shared explain module.
- **CodeClimate severity mapping for production coverage** -- `safe_to_delete` → `critical`, `review_required` → `major`, everything else → `minor` (GitLab Code Quality only renders inline MR annotations for blocker/critical/major/minor).

### Changed

- **`schema_version` bumped 3 → 4** on `fallow health --format json` -- the `production_coverage` object is reshaped to the 0.2 contract. Renames: `functions_total` → `functions_tracked`, `functions_called` → `functions_hit`, `functions_never_called` → `functions_unhit`, `functions_coverage_unavailable` → `functions_untracked`, `percent_dead_in_production` → `coverage_percent` (inverted semantics). Findings now carry `id`, required `line`, `verdict` (per-finding: `safe_to_delete` / `review_required` / `coverage_unavailable` / `low_traffic` / `active`), `invocations` (nullable for untracked functions), and a full `evidence` block. Hot paths carry `id`, required `line`, and `percentile`. Summary adds `trace_count`, `period_days`, `deployments_seen`.
- **Baseline dedup for production-coverage findings** keys on the sidecar's stable content-hash ID (`fallow:prod:<hash>`) rather than a derived `path:function:line:state` string. Moving a function now correctly shows as a fresh finding instead of a stable baseline match.

### Fixed

- **GitLab CODEOWNERS section headers (`[Docs]`, `^[Frontend]`) now parsed correctly** -- previously treated as regular rules, which produced nonsensical "reviewer: @Frontend" output. Section headers are now recognised and their nested rules attributed to the section. Section-level exclusions (`^[Frontend] @excluded`) are also respected.
- **`~` home-directory resolution on Windows** for `$FALLOW_LICENSE_PATH` and `~/.fallow/license.jwt` -- previously failed with "user home directory not found" on Windows where `$HOME` is unset by default. Now falls back to `%USERPROFILE%`.
- **`fallow health` exit code triggers on `SafeToDelete` + `ReviewRequired` verdicts** (replacing the 0.1 `NeverCalled` state) so CI continues to fail on definitive dead-code signals.
- **Numerous production-coverage hardening fixes** from the Phase 2 beta cycle: license-gate parity between CLI and sidecar, runtime license-key override removal, local license verification key support, coverage input detection polish, CI stabilization for Rust 1.95, sidecar setup recipe improvements, and protocol-version enforcement keyed on `0.<minor>` while the contract stays on 0.x.

### Breaking

- **`fallow health --production-coverage` JSON output** has moved from the 0.1 shape to the 0.2 shape described above (`schema_version 3 → 4`). Consumers of the `production_coverage` object must update field names and switch from the per-finding `state` enum (`called` / `never-called` / `coverage-unavailable`) to the `verdict` enum (`safe_to_delete` / `review_required` / `coverage_unavailable` / `low_traffic` / `active`). The top-level report `verdict` is unchanged (`clean` / `hot-path-changes-needed` / `cold-code-detected` / `license-expired-grace`).
- **`fallow-cov-protocol` requires `StaticFunction::static_used` + `test_covered`** -- any existing non-Fallow consumer of the protocol crate must populate both fields on every `StaticFunction`. The sidecar rejects 0.1-shape requests at deserialization (exit code 2) rather than silently defaulting to "used + covered", which would hide every `safe_to_delete` finding.

## [2.38.0] - 2026-04-15

### Added

- **Ownership risk analysis on hotspots** -- `fallow health --hotspots --ownership` surfaces per-file ownership signals derived from git author history and CODEOWNERS. For each hotspot: Avelino truck factor (`bus_factor`), contributor count, top contributor with stale-days and commits, up to three additional recent contributors, `suggested_reviewers` (recent contributors with `stale_days < 90` — first-class field for AI agent routing), declared CODEOWNERS owner, tristate `unowned` flag (`true`=no rule matches / `false`=rule matches / `null`=no CODEOWNERS file), drift detection with human-readable reason. Human output prepends a project-level summary line (`9/10 hotspots depend on a single recent contributor · top authors: @alice (6), @bob (4)`) so tech leads see the organizational pattern before scanning per-file rows. JSON gains three new action types: `low-bus-factor` (with file-specific candidate reviewers from `suggested_reviewers`), `unowned-hotspot` (with synthesized CODEOWNERS pattern + `heuristic: "directory-deepest"` discriminator for future evolution), and `ownership-drift`. Test-path hotspots tagged `[test]` in human output and `is_test_path: true` in JSON. Only the free tool combining dead code + complexity + ownership risk. Research-backed (Avelino et al., Thongtanunam et al. ICSE 2016). ([#116](https://github.com/fallow-rs/fallow/discussions/116))
- **`--ownership-emails={raw|handle|hash}` privacy control** -- chooses how author emails are rendered in output. `handle` (default) shows the local-part only, unwrapping GitHub-style noreply prefixes (`12345+alice@users.noreply.github.com` → `alice`); `hash` emits stable non-cryptographic `xxh3:<16hex>` pseudonyms for regulated environments where author identities are sensitive in CI artifacts (SARIF, code-scanning uploads); `raw` shows full email addresses for public OSS repos. `ContributorEntry` gains a `format` discriminator so typed JSON consumers can branch without re-parsing the identifier. Configure the repo-wide default via `health.ownership.emailMode` in config.
- **`health.ownership` config section** -- `botPatterns` (glob patterns matched against raw author emails; defaults cover `*\[bot\]*`, `dependabot*`, `renovate*`, `github-actions*`, `svc-*`, `*-service-account*` — `*noreply*` is deliberately NOT a default because it would filter real human contributors using GitHub's privacy-default email format), `emailMode` (raw/handle/hash).
- **Ownership signals in MCP `check_health` tool** -- new `ownership: bool` and `ownership_email_mode: "raw"|"handle"|"hash"` params (typed enum so JSON Schema constrains input at the agent layer). Tool description updated.

### Changed

- **Churn layer tracks per-author contributions** -- `git log` now runs with `--use-mailmap` and the `%at|%ae` format to record commit authors. Per-file `FileChurn.authors` map (`u32` → `AuthorContribution { commits, weighted_commits, first_commit_ts, last_commit_ts }`) references interned author emails in a shared pool on `ChurnResult` to keep the bitcode cache compact. Cache schema version bumped to 2; older caches are automatically rejected on load.

## [2.37.0] - 2026-04-15

### Added

- **Nx plugin now extracts `styles`, `scripts`, and `browser` entry points from `project.json`** -- Nx projects using the `@angular/build:application` executor declare global stylesheets (`styles: ["src/styles.scss"]`), polyfills/analytics scripts (`scripts: [...]`), and the Angular 17+ `browser` entry (replaces `main`) in `project.json`. Previously only `main` and `tsConfig` were picked up, so `src/styles.scss` and similar files were falsely reported as unused. Mirrors the existing Angular plugin's extraction of the equivalent `angular.json` fields. `{projectRoot}` / `{workspaceRoot}` tokens are expanded for these fields too. (Follow-up on [#125](https://github.com/fallow-rs/fallow/issues/125))

### Fixed

- **`ignorePatterns` now excludes workspace `package.json` discovery** -- `.fallowrc.json` with `"ignorePatterns": ["**/dist/**"]` correctly excludes source files from analysis but previously still scanned `dist/package.json` (build artifacts from ng-packagr, tsc, Rollup) for unused-/unlisted-dependency detection, producing a false positive for every dependency listed there. Workspace `package.json` discovery now consults the same glob filter used by the source walker. ([#124](https://github.com/fallow-rs/fallow/issues/124))
- **SCSS `@import` / `@use` bare specifiers now resolve from `node_modules/`** -- `@import 'bootstrap/scss/functions'` previously failed with `./bootstrap/scss/functions` unresolved because the resolver searched only the importing file's directory and framework-supplied `includePaths`. Matches Sass's own resolution: walks up from the importer to the filesystem root, probing each ancestor's `node_modules/` for `<pkg>/<path>.{scss,sass,css}`, partial convention (`_<name>.{scss,sass}`), and directory index (`<path>/{_index,index}.{scss,sass}`). Handles both SCSS partials (`bootstrap/scss/functions` → `_functions.scss`) and CSS fallback (`animate.css/animate.min` → `animate.min.css`). Matched packages are tracked as used so dependency analysis stays accurate. ([#125](https://github.com/fallow-rs/fallow/issues/125))
- **Angular CLI object-form `styles`/`scripts`/`polyfills` entries now extracted** -- the Angular workspace schema allows two forms per entry: the shorthand `"src/styles.scss"` and the object form `{ "input": "src/theme.scss", "bundleName": "theme", "inject": false }` (used for vendor stylesheets that opt out of auto-injection). The shared `expression_to_string_or_array` config-parser helper previously silently dropped object-form array elements, causing false-positive "unused file" reports for any file referenced that way. Both the Angular (`angular.json`) and Nx (`project.json`) plugins benefit. ([#126](https://github.com/fallow-rs/fallow/issues/126))

## [2.36.0] - 2026-04-14

### Added

- **`fallow config` subcommand** -- prints the loaded config path on the first line followed by the JSON-serialized config (with `extends` resolved). Use `--path` to print only the path (scriptable). Exit code 0 if a config was found, 3 if only defaults are in effect. Mirrors `eslint --print-config` and `dprint output-resolved-config`.
- **`loaded config: <path>` line on stderr** -- the CLI now prints which config file was picked up at the start of every human-format run (suppressed by `--quiet` and non-human formats). Closes the silent-config-loss UX gap where users could not tell whether their `.fallowrc.json` was being applied. The LSP emits the same information as a `window/logMessage` per project root, visible in the editor's Output panel.
- **`sealed: true` config field** -- marks a config as self-contained so `extends` paths must be file-relative and resolve within the config's own directory. `npm:` and `https:` extends are rejected. Useful for library publishers and monorepo sub-packages that want to guarantee their config cannot be modified by ancestor configs being injected via `extends`.

### Fixed

- **Config discovery now walks past `package.json` in monorepos** -- config search previously stopped at the first `package.json`, silently preventing monorepo sub-packages (pnpm/npm/yarn workspaces, Nx) from inheriting the root `.fallowrc.json`. Discovery now stops only at VCS boundaries (`.git`, `.hg`, `.svn`), matching Prettier/ESLint/Biome behavior. This fixes silent config loss in the VS Code extension, LSP, and CLI when invoked from a workspace sub-package. A sub-package with its own `.fallowrc.json` still wins (first-match-wins). ([#113](https://github.com/fallow-rs/fallow/issues/113))

## [2.35.0] - 2026-04-14

### Added

- **Severity tiers on health findings** -- each complexity finding now has a `severity` field (`moderate`, `high`, `critical`) based on how far above thresholds. Default thresholds: cognitive 25/40, cyclomatic 30/50 (asymmetric because cognitive maps more directly to comprehension difficulty). Summary includes `severity_critical_count`, `severity_high_count`, `severity_moderate_count`. Human output shows `CRITICAL` and `HIGH` badges on findings. All 6 output formats updated. ([#119](https://github.com/fallow-rs/fallow/issues/119))
- **`--min-severity` CLI flag** -- only exit with error for findings at or above the specified severity level (`moderate`, `high`, `critical`). Use `--min-severity critical` to ignore moderate/high findings in CI.

### Fixed

- **Maintainability Index formula distorted for small files** -- the MI formula used `complexity_density = cyclomatic / LOC` as its dominant term, causing a 5-line type-guard function to score worse than a 192-line function with 57 cyclomatic complexity. Added a LOC dampening factor `min(lines/50, 1.0)` that proportionally reduces the density weight for files under 50 lines. `SNAPSHOT_SCHEMA_VERSION` bumped to 7. ([#118](https://github.com/fallow-rs/fallow/issues/118))
- **Integration test fixture for bin-script dependency resolution** tracked in git.

## [2.34.0] - 2026-04-14

### Added

- **Per-file LOC in health output** -- surfaces lines of code per file in health analysis, enabling size-aware codebase navigation.
- **Total LOC vital sign** -- adds total lines of code as a health vital sign with penalty breakdown visibility.

### Changed

- **Significant performance improvements** -- parallel file walker via `ignore` crate, O(1) export name dedup, O(n^2) elimination in analyze/cross-reference/families, shared parsed modules and analysis graph between check and health in combined mode, borrowed line offsets in complexity computation.

### Fixed

- **CLI binary names in scripts resolved via dependency `bin` fields** -- packages whose binary name diverges from the package name (e.g., `@arethetypeswrong/cli` provides `attw`) are no longer falsely flagged as unused dependencies when used in npm scripts. Builds a reverse map from dependency `package.json` `bin` entries, probing `node_modules/` at project root and workspace roots. ([#107](https://github.com/fallow-rs/fallow/discussions/107))
- **Dynamic import `.then()` member access tracked** -- `import('./Foo').then(m => m.Bar)` now correctly credits `Bar` as a used export, preventing false unused-export reports for lazy-loaded named exports. ([#115](https://github.com/fallow-rs/fallow/issues/115))
- **Nx workspace tokens expanded in all project.json fields** -- `{projectRoot}` and `{workspaceRoot}` tokens are now expanded in `main` and `tsConfig` fields, not just `includePaths`. ([#114](https://github.com/fallow-rs/fallow/issues/114))
- **Deterministic clone family sort** -- added tiebreaker to clone family ordering to prevent non-deterministic output.

## [2.33.0] - 2026-04-13

### Added

- **`ignoreImports` config option** -- excludes ES `import` declarations from clone detection, reducing noise from sorted import blocks that naturally look similar across files. Available as `ignoreImports: true` in the `duplicates` config section and `--ignore-imports` CLI flag on the `dupes` command. Only affects ES import statements; CommonJS `require()` calls and re-exports are unaffected. Default: `false`. ([#113](https://github.com/fallow-rs/fallow/issues/113))

### Fixed

- **`fallow audit` now respects project duplication config** -- the `audit` command previously hardcoded default values for duplication settings (`mode`, `minTokens`, `minLines`, `skipLocal`, `crossLanguage`), ignoring user configuration. Now reads from the project config file, matching the behavior of `fallow` (combined mode) and `fallow dupes`.
- **Hardened CI workflow permissions** -- tightened GitHub Actions permissions for OpenSSF Scorecard compliance.

## [2.32.1] - 2026-04-13

### Fixed

- **Test-only dependency detection now recognizes workspace-level config files** -- after #111 narrowed config file exclusion patterns to root-only, `find_test_only_dependencies` no longer matched nested config files like `packages/foo/vitest.config.ts`. Now uses the curated `is_config_file()` predicate alongside glob patterns, correctly identifying workspace-level tooling configs without false-matching application configs like `app.config.ts`. ([#112](https://github.com/fallow-rs/fallow/issues/112))

## [2.32.0] - 2026-04-13

### Added

- **Stale suppression detection** -- detects orphaned `// fallow-ignore-next-line` and `// fallow-ignore-file` comments that no longer match any active issue. Implemented via a `SuppressionContext` that tracks which suppressions are consumed by detectors during analysis. Default severity: `warn` (reports without failing CI). Non-core suppression kinds (complexity, feature flags, code duplication, dependency-level) are excluded from stale detection to avoid false positives. New `--stale-suppressions` CLI flag to filter output to stale findings only. Full support across all 6 output formats, LSP diagnostics (`HINT` severity with `UNNECESSARY` tag), MCP server, GitHub Action annotations, and GitLab CI summaries.
- **`@expected-unused` JSDoc tag** -- marks an export as intentionally unused. Unlike `@public` (permanent exemption), `@expected-unused` reports a stale suppression when the export becomes used (gains references from reachable modules). Useful for API completeness patterns where exports exist for symmetry but have no internal consumers yet.
- **Structured suppression origin in JSON output** -- stale suppression findings include a discriminated `origin` field (`"type": "comment"` with `issue_kind` and `is_file_level`, or `"type": "jsdoc_tag"` with `export_name`) for machine consumers to distinguish inline comments from JSDoc tags.

### Fixed

- **Health test no longer fails with global git signing config** -- isolated temp repo git operations from global config (`GIT_CONFIG_GLOBAL=/dev/null`) to prevent commit signing requirements from breaking the `--changed-since` integration test.

## [2.31.0] - 2026-04-13

### Added

- **5 new framework plugins** -- Hardhat, UnoCSS, Qwik, Convex, and pnpm are now detected automatically (85 to 90 total plugins).
- **LSP binary distributed via npm** -- `fallow-lsp` is now included in the npm package alongside `fallow` and `fallow-mcp`. The VS Code extension resolves the LSP binary from `node_modules/.bin/fallow-lsp` without a separate install step.

### Fixed

- **`production: true` no longer excludes Angular `app.config.ts`** -- the `**/*.config.*` production exclude pattern was too broad, matching Angular's `src/app/app.config.ts` (a runtime application file) and breaking the entire import chain. Narrowed to `*.config.*` (root-anchored) with `literal_separator(true)` so nested config files in `src/` are preserved. Also added `app.config.ts` and `app.config.server.ts` to the Angular plugin's `always_used` list as defense-in-depth. ([#111](https://github.com/fallow-rs/fallow/issues/111))
- **Health test no longer fails with global git signing config** -- isolated temp repo git operations from global config (`GIT_CONFIG_GLOBAL=/dev/null`) to prevent commit signing requirements from breaking the `--changed-since` integration test.

## [2.30.0] - 2026-04-12

### Added

- **Arrow-wrapped dynamic import detection** -- `React.lazy(() => import('./Foo'))`, `loadable(() => import('./Component'))`, `defineAsyncComponent(() => import('./View'))`, and any function call with an arrow or function expression argument whose body is a sole `import()` expression now credits the target module's default export as used. Handles expression body, block body with return, and function expressions. Covers all lazy loading wrappers generically without maintaining a list of known functions.
- **JSDoc visibility tags** (`@internal`, `@beta`, `@alpha`) -- extends the existing `@public` tag support with three additional TSDoc lifecycle tags. All four tags suppress unused-export detection while preserving semantic distinction via a `VisibilityTag` enum (`Public`, `Internal`, `Alpha`, `Beta`). Priority when multiple tags present: Public > Internal > Alpha > Beta. Stored across the full pipeline (extraction, cache, graph, analysis). `#[repr(u8)]` with explicit discriminants for stable cache encoding.
- **`--include-entry-exports` flag** -- makes entry file exports (package.json main/module/exports, framework convention files) subject to unused-export detection instead of automatically marking them as used. Catches typos in framework exports (e.g., `meatdata` instead of `metadata` in Next.js pages). Still respects plugin `used_exports` rules for partial checking.
- **`--file <PATH>` flag** -- scopes check output to specified file(s) for lint-staged pre-commit integration. Accepts multiple paths. Full graph still builds (correct results require cross-file analysis), but only issues in matching files are reported. Dependency-level issues are suppressed in file mode. Warns when a path is not found (showing the resolved path).
- **Script multiplexer support** -- `concurrently`, `npm-run-all`, `npm-run-all2`, `run-s`, `run-p`, `run-s2`, `run-p2` are recognized as script multiplexers. Their positional arguments are treated as script names (not binary dependencies), preventing false "unused dependency" reports for packages referenced only via `npm:` prefixed script delegation.

### Fixed

- **Arrow-wrapped dynamic imports now credit default export** -- `ImportedName::Named("default")` from the arrow-wrapped import extractor is now correctly resolved to `ImportedName::Default`, so lazy-loaded modules have their default export credited in the module graph. Previously, the default export appeared unused despite the file being reachable.

## [2.29.1] - 2026-04-12

### Fixed

- **Feature flag inline suppression now works** -- `// fallow-ignore-next-line feature-flag` and `// fallow-ignore-file feature-flag` now suppress individual or all feature flag findings in the `fallow flags` command. Previously, the JSON output suggested this comment but the command never checked the suppression system, so the comment was silently ignored. Both the built-in detection loop and the custom SDK/env-prefix detection loop now call `is_suppressed()` / `is_file_suppressed()` with `IssueKind::FeatureFlag`.

## [2.29.0] - 2026-04-12

### Added

- **Inline suppression for health complexity findings** -- `// fallow-ignore-next-line complexity` and `// fallow-ignore-file complexity` now suppress individual functions or entire files from the health command's complexity findings. The blanket `// fallow-ignore-next-line` (without a kind) also works. Useful for intentionally complex code like generated parsers or configuration dispatch functions that you don't want to refactor but don't want cluttering health output. File scores, vital signs, and hotspot rankings are unaffected by suppressions since they reflect actual complexity, not alerting. The `IssueKind::Complexity` variant (discriminant 18) integrates with the existing suppression system used by all other issue types. Human output now shows a suppress hint in the findings footer when 3+ functions exceed thresholds. ([#108](https://github.com/fallow-rs/fallow/issues/108))
- **Standalone plugin schema** -- `fallow config-schema --plugins` emits a JSON schema describing all built-in plugin names, useful for editor autocompletion in config files. ([#109](https://github.com/fallow-rs/fallow/pull/109))

## [2.28.2] - 2026-04-11

### Fixed

- **`` html`...` `` tagged template literal asset references** -- `<script src="...">` and `<link rel="stylesheet|modulepreload" href="...">` inside the `html` tagged template literal (used by [Hono](https://hono.dev) via `hono/html`, [lit-html](https://lit.dev), and [htm](https://github.com/developit/htm)) now emit `SideEffect` imports via a new `visit_tagged_template_expression` override, mirroring the JSX `<script src>` / `<link href>` fix shipped in 2.28.1. The bare identifier tag `html` is matched only — `css`, `sql`, `gql`, `styled.div`, and member or call expressions are deliberately ignored so unrelated tagged templates in the same file are never misread as HTML. Each template quasi is scanned independently with the same regex pipeline used by the HTML file parser (extracted into a shared `collect_asset_refs` helper), so an asset reference split across an interpolation boundary (`` html`<script src="${base}/app.js">` ``) is skipped rather than producing a garbled, unresolvable specifier. Addresses till's follow-up comment on [#105](https://github.com/fallow-rs/fallow/issues/105) where `.ts` Hono layouts using the `html` tagged template (rather than JSX) still flagged sibling `static/*.js` files as unused.

## [2.28.1] - 2026-04-11

### Fixed

- **JSDoc `import()` type references now tracked as type-only imports** -- types referenced only from JSDoc type annotations (e.g., `@param foo {import('./types.js').Foo}`, `@returns`, `@type`, `@typedef`, `@callback`) are no longer flagged as unused exports. A new comment scanner runs alongside the existing `@public` tag extractor in `parse_source_to_module`, pushing synthesized `ImportInfo` entries with `is_type_only: true` and empty `local_name` so the module graph treats the referenced exports as used and the referenced files as reachable. Covers all JSDoc tag contexts, union annotations (`{import('./a').A | import('./b').B}`), nested member access, bare specifiers, parent-relative paths, and side-effect-only `import('./types')` without a `.Member`. Only `/** */` JSDoc blocks are scanned; single-star `/* */` comments are ignored. Addresses the first gap raised by till on [#105](https://github.com/fallow-rs/fallow/issues/105).
- **JSX `<script src>` / `<link href>` asset references inside JSX/TSX files** -- lowercase intrinsic `<script src="...">` and `<link rel="stylesheet|modulepreload" href="...">` inside JSX/TSX now emit `SideEffect` imports via a new `visit_jsx_opening_element` override, mirroring the HTML parser at the JSX level. Needed for SSR frameworks like [Hono](https://hono.dev) where layout components emit HTML via JSX and assets are referenced as plain string literals. Only `StringLiteral` attribute values are captured; `{expr}` containers and capitalized React-style components (`<Script>`, `<Link>`) are intentionally skipped. The web-root-relative resolver branch is also broadened from HTML-only to cover `jsx|tsx|js|ts|mjs|cjs|mts|cts` sources so `<link href="/static/style.css" />` resolves against the source file's parent directory first and the project root second. Addresses the second gap raised by till on [#105](https://github.com/fallow-rs/fallow/issues/105).

## [2.28.0] - 2026-04-11

### Added

- **Configurable class member allowlist** -- new top-level `usedClassMembers` config field and per-plugin `usedClassMembers` field on external plugins extend the built-in Angular/React lifecycle allowlist with framework-invoked method names. Useful for libraries that call consumer methods reflectively through an interface contract: ag-Grid's `agInit`/`refresh`, TypeORM's `MigrationInterface.up`/`down`, Web Components' `connectedCallback`/`disconnectedCallback`/`attributeChangedCallback`, and any strategy/plugin pattern. The allowlist is merged with plugin contributions during analysis and scoped to class methods and properties, so enum members with matching names are still flagged. ([#98](https://github.com/fallow-rs/fallow/issues/98))
- **Angular `stylePreprocessorOptions.includePaths`** -- the Angular plugin now reads `projects.*.architect.build.options.stylePreprocessorOptions.includePaths` from `angular.json`, and the Nx plugin reads the same key from `project.json`. Bare SCSS `@import 'variables'` / `@use 'mixins'` specifiers that fail file-local resolution retry against each configured include directory using the SCSS partial and directory-index conventions. Eliminates false unresolved-imports and false unused-files for Angular and Nx projects that share SCSS partials through `includePaths`. Parent-relative specifiers (`../shared/vars`) are left untouched so they cannot be silently redirected. ([#103](https://github.com/fallow-rs/fallow/issues/103))

### Fixed

- **Self-referencing and cross-workspace package imports in monorepo libraries** -- when a workspace library uses its own name in import specifiers (a [Node.js v12+ feature](https://nodejs.org/api/packages.html#self-referencing-a-package-using-its-name) commonly used by Angular libraries built with `ng-packagr`), secondary entry point files are no longer reported as unused. A new workspace package fallback strips the matching package name prefix from any bare specifier whose package name is in the workspace registry and resolves the remainder against the library's source tree via `oxc_resolver::resolve_file`, bypassing the `package.json` `exports` map's pointers at compiled output. The fallback also covers cross-workspace imports in monorepos that haven't run `npm install`, where bare `@org/pkg/sub` specifiers would previously fall through to `NpmPackage` classification and create false-positive unused-file and unlisted-dependency reports. ([#106](https://github.com/fallow-rs/fallow/issues/106))
- **SCSS `sass:*` built-in modules no longer reported as unlisted npm dependencies** -- the Sass compiler's [built-in modules](https://sass-lang.com/documentation/modules/) imported via `@use 'sass:string'`, `@use 'sass:math'`, etc. are now recognized alongside the existing `node:`, `bun:`, `cloudflare:`, and Deno `std` platform builtins. These are language-level Sass features, not npm packages, and cannot appear in `package.json`. Previously, every unique `sass:*` module used across a codebase produced a separate false-positive unlisted dependency report (up to 7 per Sass codebase for the full module set: `math`, `string`, `color`, `list`, `map`, `meta`, `selector`). The prefix guard is strict: `sass-loader`, `@types/sass`, and the `sass` compiler package itself remain normal npm packages. ([#104](https://github.com/fallow-rs/fallow/issues/104))
- **Entry points inside ignored output directories now fall back to source index** -- when a `package.json` `main`/`module`/`exports` field resolves to a file inside an ignored output directory (`dist`, `build`, `out`, `esm`, `cjs`) and no direct source mirror exists in `src/`, fallow now probes `src/index.*`, `src/main.*`, `index.*`, `main.*` in the package root. Prevents TypeScript libraries whose compiled entry has no one-to-one source mapping (e.g. `dist/esm2022/index.js` with only `src/index.ts`) from having their entire source tree marked unreachable. Emits a single `tracing::info!` per fallback so the heuristic is visible in logs. ([#102](https://github.com/fallow-rs/fallow/issues/102))

## [2.27.6] - 2026-04-11

### Fixed

- **Bare HTML / Vue / Svelte asset references without `./` prefix** -- HTML `<script src="app.js">`, `<link rel="stylesheet" href="styles.css">`, `<link rel="modulepreload" href="vendor.js">`, and Vue/Svelte `<script src="logic.ts">` now normalize bare filenames to `./`-prefixed relative paths before downstream resolution. Previously these were reported as unlisted npm dependencies because the resolver's specifier classifier treats anything without `./`, `/`, or `://` as a bare package. Browsers, Vite, Parcel, and the Vue/Svelte compilers all resolve these references relative to the document or component file whether or not the `./` prefix is present, so normalization matches real-world semantics. Scoped package paths (`@scope/header.html`), root-absolute paths (`/src/main.ts`), URLs, and `data:` URIs remain untouched. Same bug shape as the Angular `templateUrl` fix shipped in 2.27.5. ([#101](https://github.com/fallow-rs/fallow/pull/101))

## [2.27.5] - 2026-04-11

### Fixed

- **Import-then-reexport pattern recognized as re-export** -- the pattern `import { X } from './a'; export { X };` is now detected as a re-export (semantically equivalent to `export { X } from './a';`) instead of a local export. The visitor scans previously-seen imports when processing an export without a `from` clause and emits a `ReExportInfo` pointing back to the original source. Eliminates false duplicate-export findings (where the bottom export collided with the original) and false unused-export reports (where the re-export chain propagation never reached the misclassified entry). Handles renames on either side, default imports, and mixed local + re-export statements. Order-sensitive: relies on imports preceding exports in source order.
- **Accurate line numbers for unused re-exports** -- re-exports synthesized into `ExportSymbol` entries from `ReExportInfo` were reported with line `:1` because the synthesis used `Span::new(0, 0)` as a sentinel. The visitor's real source span is now plumbed through `ReExportInfo` and `ReExportEdge` into the synthesized `ExportSymbol`, so unused re-exports now report the actual line and column of the `export {...}` declaration. The `(re-export)` marker in human/JSON/SARIF/CodeClimate output is preserved via a semantic lookup against the module's `re_exports` list. On vue-core, 86 unused re-export entries went from `:1` to their real source positions.
- **Angular `templateUrl` / `styleUrl` without `./` prefix** -- bare filenames in Angular `@Component` decorators (e.g., `templateUrl: 'app.component.html'`) are now treated as relative paths instead of being misclassified as npm package specifiers. Angular resolves both `'app.component.html'` and `'./app.component.html'` relative to the component file, so the `./` prefix is optional. Without normalization, bare filenames were reported as unlisted dependencies. Already-relative, absolute, URL, and scoped-package paths are left untouched. ([#99](https://github.com/fallow-rs/fallow/issues/99), [#100](https://github.com/fallow-rs/fallow/pull/100))

### Changed

- **O(N) re-export detection** -- the new `is_re_export` lookup pre-builds an `FxHashSet` of re-exported names per module, replacing a per-export `iter().any()` scan. Avoids O(N²) work on barrel files with many re-exports.

## [2.27.4] - 2026-04-10

### Fixed

- **Broken tsconfig chain no longer poisons sibling resolution** -- a solution-style `tsconfig.json` with `references` to a sibling tsconfig that has a broken `extends` chain (e.g., extending a non-existent file, as commonly scaffolded by Angular CLI / Nx workspaces) no longer fails import resolution for files covered by a healthy sibling. Previously, a broken `tsconfig.spec.json` (covering only `*.spec.ts`) caused every relative import in the workspace to be flagged as unresolved, even in files covered by a valid `tsconfig.app.json`. Fallow now retries resolution without tsconfig context when it detects a tsconfig-loading error (`TsconfigNotFound`, `TsconfigCircularExtend`, `TsconfigSelfReference`, malformed JSON, IO error), and emits a single `tracing::warn!` per unique broken chain so the configuration issue is visible instead of silent. Path aliases in the broken workspace remain unresolvable (they were already broken in `tsc`). ([#97](https://github.com/fallow-rs/fallow/issues/97))
- **VS Code extension packaging** -- the root `LICENSE` file is now copied into `editors/vscode/` at package time so it is bundled in the VSIX. `vsce` only reads the extension's working directory, so the repo-root `LICENSE` was previously not included in the published extension.

### Changed

- **VS Code extension size reduced 48%** -- `.vscodeignore` now excludes `pnpm-lock.yaml`, `.fallow/`, `.test-dist/`, `test/`, `vitest.config.mts`, `tsconfig.test.json`, and `.fallowrc.json`. The packaged VSIX is 100 KB (down from 192 KB).
- **CSS path alias regression tests** -- added coverage for `@/components/Button.css` sharing the `@`-prefix with scoped packages, ensuring path aliases stay as bare specifiers for the resolver's alias fallback to handle.

## [2.27.3] - 2026-04-10

### Fixed

- **CSS scoped package imports** -- `@import` statements referencing scoped npm packages with CSS extensions (e.g., `@fontsource/monaspace-neon/400.css`, `@fallow/design-system/styles.css`) are now kept as bare specifiers for node_modules resolution. Previously, the extension-based heuristic incorrectly prepended `./`, causing all such imports to be reported as unresolved.
- **HTML root-relative paths in workspace members** -- `<script src="/src/main.tsx">` in workspace member HTML files (e.g., `site/index.html`) now resolves relative to the workspace member root, not the monorepo root. Previously, `site/index.html` referencing `/src/main.tsx` looked for `<monorepo>/src/main.tsx` instead of `site/src/main.tsx`, leaving the entry point file and all its transitively reachable modules flagged as unused. Resolution now tries the HTML file's parent directory first, with fallback to the project root for the `public/index.html` pattern.
- **SCSS directory index convention** -- `@use 'components'` now resolves to `components/_index.scss` or `components/index.scss` per SCSS directory-import convention, in addition to the existing `_components.scss` partial fallback.
- **`export default class extends Foo` inheritance** -- default-exported classes with an `extends` clause now participate in the inheritance-aware class member detection introduced in 2.27.2, preventing false unused-member reports on child class overrides.

## [2.27.2] - 2026-04-10

### Fixed

- **SCSS partial resolution** -- `@use 'variables'` and `@import '../styles/variables'` now resolve to `_variables.scss` per SCSS convention. Extensionless bare specifiers in `.scss`/`.sass` files are normalized to relative paths, and the resolver tries the underscore-prefix fallback before npm package classification. Eliminates cascading false positives across unresolved imports, unlisted dependencies, and unused files. ([#95](https://github.com/fallow-rs/fallow/issues/95))
- **Inheritance-aware class member detection** -- child class members accessed via `this.*` in parent class methods are no longer flagged as unused. Builds an inheritance map from `extends` clauses and propagates member accesses bidirectionally through the hierarchy. Reduces false positives from 7 to 1 in typical polymorphic dispatch patterns (the remaining case requires type inference). Also handles `export default class extends Foo` patterns. ([#96](https://github.com/fallow-rs/fallow/issues/96))

## [2.27.1] - 2026-04-10

### Fixed

- **Angular `@defer` block scanning** -- `@defer (when condition)` expressions are now parsed for member references, including `prefetch when` and `hydrate when` variants. Previously, members referenced only in `@defer` conditions were flagged as unused. ([#94](https://github.com/fallow-rs/fallow/issues/94))
- **Angular `@let` template variables** -- `@let name = expr;` template-local variable declarations are now parsed, extracting member references from the expression and tracking the variable as a template local. Previously, members referenced only in `@let` expressions were flagged as unused. ([#93](https://github.com/fallow-rs/fallow/issues/93))
- **Angular `@else if` condition scanning** -- `@else if (condition)` expressions are now parsed for member references. Previously, the condition was skipped and members referenced only in `@else if` were flagged as unused.
- **Angular `outputFromObservable()` recognition** -- class properties initialized with `outputFromObservable()` (from `@angular/core/rxjs-interop`) are now recognized as framework-managed, matching the existing `output()` handling. ([#92](https://github.com/fallow-rs/fallow/issues/92))

## [2.27.0] - 2026-04-10

### Added

- **Feature flag detection** -- new `fallow flags` command detects static feature flags and environment gates in JavaScript/TypeScript code. Three detection categories: boolean flags (`if (FLAG)`), environment checks (`process.env.NODE_ENV`), and config-key guards (`config.featureX`). Cross-references with dead code analysis to surface flags guarding unused code. All 6 output formats supported (human, JSON, compact, SARIF, Markdown, CodeClimate). Available as MCP tool for AI agent integration.
- **Deep Docusaurus convention coverage** -- the Docusaurus plugin now parses `docusaurus.config.*` to discover content plugin instances (docs, blog, pages), classic preset expansion, theme swizzle components, i18n/versioned docs, static directories, client modules, scripts, and stylesheets. Sidebar files are only marked as used when explicitly referenced via `sidebarPath`. Contributed by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza). ([#91](https://github.com/fallow-rs/fallow/pull/91))

### Changed

- **Plugin infrastructure** -- 9 plugins migrated from manual `impl Plugin` to the `define_plugin!` macro, reducing boilerplate. Shared config parser helpers extracted for reuse across plugins (function body extraction for async config creators, property accessors, disabled-section detection).
- **MCP server** -- shared CLI arg-building helpers extracted, reducing duplication across tool implementations.
- **Internal visibility** -- core crate modules restricted to `pub(crate)` where cross-crate access was unnecessary.
- **SFC template parser** -- deduplicated template tag parsing logic shared between Vue and Svelte extractors.

### Fixed

- **VS Code extension** -- version marker round-trip handling improved with additional test coverage.

## [2.26.1] - 2026-04-09

### Changed

- **Health: churn disk cache and parallel git log** -- churn analysis results are now cached to disk and git log calls run in parallel, significantly reducing health command runtime on repos with long histories. New `--performance` flag prints timing breakdown per analysis phase.

### Fixed

- **VS Code extension: stale LSP binary after extension update** -- the version check that detects outdated auto-downloaded binaries silently skipped re-downloading when `--version` output could not be parsed, leaving a stale binary in place. Now writes a `.fallow-version` marker file alongside downloaded binaries and treats unknown-version binaries as stale. Also warns when a PATH-resolved binary is outdated relative to the extension. ([#90](https://github.com/fallow-rs/fallow/issues/90))
- **GitHub Action: multiple JSON objects in bare invocation** -- `analyze.sh` now handles bare invocations that produce multiple JSON objects instead of failing to parse.

## [2.26.0] - 2026-04-09

### Added

- **Expo Router plugin** -- new `expo-router` plugin (85th built-in) detects Expo Router projects, reads the configured route root from `app.json` / `app.config.ts`, and marks route files plus special-file exports as framework-used. Covers `+api` (HTTP methods), `+middleware`, `+html`, `+not-found`, and `+native-intent` conventions. Contributed by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza). ([#88](https://github.com/fallow-rs/fallow/pull/88))
- **TanStack Router config parsing** -- the `tanstack-router` plugin now reads `tsr.config.json` to support custom route directories (`routesDirectory`), route file prefixes (`routeFilePrefix`), ignore patterns (`routeFileIgnorePrefix`, `routeFileIgnorePattern`), generated route tree location (`generatedRouteTree`), and lazy route files (`.lazy.tsx`). Config-driven routes replace static defaults instead of layering. Contributed by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza). ([#88](https://github.com/fallow-rs/fallow/pull/88))
- **Path rule exclusions** -- plugin entry-point and used-export rules now support glob, regex, and segment-regex exclusions for strict file matching. Prevents false negatives from overly broad patterns.

### Fixed

- **GitHub Action: fork PR CI failures** -- the `analyze.sh` script inherited `set -e` (errexit) from the composite action runner shell, causing silent failures on fork PRs where intermediate commands (git diff, grep) return non-zero as part of normal flow.
- **LSP: workspace diagnostic merging** -- `test_only_dependencies`, `boundary_violations`, and `export_usages` are now correctly merged across workspace roots instead of being dropped.

## [2.25.1] - 2026-04-09

### Fixed

- **VS Code extension: duplicate diagnostics in multi-root workspaces** -- diagnostics were shown N times (once per workspace root) when hovering over issues. Fixed by building diagnostics once from the merged results instead of per-root. ([#90](https://github.com/fallow-rs/fallow/issues/90))

## [2.25.0] - 2026-04-09

### Added

- **Health: large function drill-down and duplication penalty** -- `fallow health` now shows the largest functions contributing to unit size risk and applies a score penalty when code duplication is detected. Progressive disclosure hints guide users to relevant flags (`--top`, duplication timing).
- **SIG-aligned clippy lints and module coupling CI** -- added `too_many_lines` (150 LOC), `too_many_arguments` (7 params), and `cognitive_complexity` (25) thresholds aligned with SIG maintainability properties. New CI workflow tracks inter-module coupling metrics.

### Fixed

- **VS Code extension: raw ANSI escape codes in Output panel** -- the LSP server's tracing subscriber now disables ANSI color codes, matching the pattern already used by the MCP server. VS Code's Output panel does not render ANSI sequences. ([#89](https://github.com/fallow-rs/fallow/issues/89))

## [2.24.0] - 2026-04-09

### Added

- **Function size and parameter risk profiles** -- `fallow health` now computes risk distribution histograms for function size (1-15 / 16-30 / 31-60 / >60 LOC) and parameter count (0-2 / 3-4 / 5-6 / >=7 params). Profiles appear in JSON as `unit_size_profile` and `unit_interfacing_profile` on `vital_signs`, and in human output when approaching penalty thresholds.
- **Parameter counting on functions** -- `FunctionComplexity` now includes `param_count` (excluding TypeScript's `this` parameter, including rest parameters). Available in health findings JSON output.
- **Coupling concentration metrics** -- `p95_fan_in` and `coupling_high_pct` added to vital signs, tracking the 95th percentile of incoming dependencies and the percentage of highly-coupled files.
- **Two new health score penalties** -- `unit_size` (max 10 points, activates when >5% of functions exceed 60 LOC) and `coupling` (max 5 points, activates when p95 fan-in exceeds 30). Conservatively calibrated: zero penalty on well-maintained projects.
- **Trend schema version migration warning** -- when comparing against a snapshot from an older schema version, the trend output warns that score deltas may reflect formula changes rather than code changes.

### Changed

- **Conditional risk profile rendering** -- function size profiles only appear in human output when the very-high-risk bin reaches 3% (approaching the 5% penalty threshold). Parameter profiles only appear when high or very-high bins carry signal. Healthy projects see clean, noise-free output.
- **Risk profiles visible during trend view** -- removed suppression of risk profiles when comparing against snapshots, so users can see absolute distributions alongside trend deltas.
- **Snapshot schema v4** -- added risk profiles and coupling fields. Old v3 snapshots load seamlessly via serde defaults.

### Fixed

- **Baseline key portability** -- baseline keys now use relative paths for cross-machine consistency.
- **Zero-match baseline warning** -- improved messaging when baselines match no current issues.
- **Baseline stats in combined JSON** -- unified baseline key helpers across check/health/dupes.

## [2.23.1] - 2026-04-09

### Fixed

- **VS Code extension: explicit `lspPath` setting now takes priority over local `node_modules`** -- the `fallow.lspPath` setting represents explicit user intent and should not be silently overridden by an implicitly discovered devDependency binary. Resolution order is now: `lspPath` > `node_modules/.bin` > PATH > global storage > auto-download.

## [2.23.0] - 2026-04-09

### Added

- **VS Code extension: local devDependency binary resolution** -- the extension now checks `node_modules/.bin/` in the workspace root for locally installed binaries. Teams can pin fallow as a devDependency for consistent versions across developers. ([#86](https://github.com/fallow-rs/fallow/discussions/86))

### Fixed

- **Infinite recursion on TypeScript type annotations in destructuring patterns** -- `collect_pattern_usage` now strips type annotations before traversing, preventing stack overflow on patterns like `({ href, content }: Props)` in Vue and Svelte templates.

## [2.22.4] - 2026-04-08

### Added

- **Nuxt module authoring support** -- projects using `@nuxt/kit` with `src/runtime/` directory now have components, composables, plugins, utils, locale, types, and vue subdirectories recognized as entry points. Tested on Nuxt UI v3: false positive unused files dropped from 521 to 226.

### Fixed

- **Stack overflow on Svelte 5 typed snippet parameters** -- `{#snippet Link({ href, content }: Props)}` caused infinite recursion in `extract_pattern_binding_names` because the TypeScript type annotation prevented destructuring pattern matching. Projects like shadcn-svelte now analyze without crashing.

### Changed

- **Duplicate detection in re-export chain resolution** -- switched from linear scan to `FxHashSet` for O(1) duplicate detection during star re-export propagation.
- **Reduced allocations in plugin system and graph construction** -- reuses scratch buffers and avoids unnecessary `String`/`Vec` allocations.

## [2.22.3] - 2026-04-08

### Fixed

- **aarch64-linux-musl release build failure** -- switched from vendored OpenSSL (`native-tls`) to `rustls` for TLS, eliminating all C dependencies for cross-compilation. Vendored OpenSSL linked against glibc symbols (`__memcpy_chk`) unavailable on musl targets.

## [2.22.2] - 2026-04-08

### Added

- **Meta-framework prepare step warning** -- warns when Nuxt, Next.js, SvelteKit, or Astro need a prepare/build step before analysis can see generated types and routes.

### Fixed

- **CI dupes comment showing "No code duplication found" with `--changed-since` active** -- duplication JSON output had absolute file paths instead of project-relative paths, causing the GitHub Action's changed-file filter to silently drop all clone groups. The same issue existed in combined and audit mode for both dupes and health sections. ([#85](https://github.com/fallow-rs/fallow/issues/85))
- **Star re-exports not propagated through entry point chains** -- `export * from './module'` in barrel files referenced by entry points now correctly marks the re-exported module's exports as used.
- **Wildcard subpath exports missing CSS/asset entry points** -- package.json `exports` maps with wildcards (`"./*": "./dist/*.js"`) now expand to discover CSS, SCSS, and asset entry points.
- **Astro `getStaticPaths` not recognized for TypeScript page files** -- `.ts` files in Astro `src/pages/` now have `getStaticPaths` marked as framework-used.
- **`.js` resolved before `.d.ts` when both exist side-by-side** -- prevents false unresolved imports when a `.js` implementation file coexists with a `.d.ts` declaration.
- **Test framework `include`/`testMatch`/`testRegex` patterns not used as entry point overrides** -- Vitest `test.include` and Jest `testMatch`/`testRegex` config patterns now correctly override the default test file discovery.
- **Case-insensitive HTML/SFC regex matching on non-ASCII** -- added `unicode-case` feature to regex crate for correct case-insensitive matching in HTML and Vue/Svelte template parsing.
- **TLS provider not configured for URL extends resolution** -- explicitly configured native-tls provider for ureq HTTP client, fixing `extends: "https://..."` config resolution.
- **OpenSSL missing on Linux musl/cross-compilation** -- added vendored OpenSSL for static linking on musl targets.

### Changed

- **Binary size reduced by 16.6%** (-1.38MB) via release profile optimizations.
- **Oxc parser upgraded from 0.123 to 0.124.**
- **Sub-packages discovered from package.json exports map** -- nested packages referenced via `exports` field subpath patterns are now resolved.

## [2.22.1] - 2026-04-08

### Fixed

- **Astro frontmatter extraction on Windows** -- the regex for extracting code between `---` delimiters required LF line endings, silently failing on Windows where git checks out CRLF. All `.astro` file exports were invisible on Windows. Now handles both LF and CRLF.

## [2.22.0] - 2026-04-08

### Added

- **Expanded Astro convention coverage** -- actions (`src/actions/index`), content config (`src/content/config`, `src/content.config`), nested middleware (`src/middleware/index`), page exports (`getStaticPaths`, `prerender`, `partial`), and endpoint HTTP method handlers (`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`, `OPTIONS`, `ALL`) are now recognized as framework-used. ([#84](https://github.com/fallow-rs/fallow/pull/84))
- **React Router v7 route config and root module exports** -- `app/routes.ts` default export, root `Layout`, `middleware`, `clientMiddleware`, and `shouldRevalidate` are now kept alive. ([#84](https://github.com/fallow-rs/fallow/pull/84))
- **Remix root and client data exports** -- `clientLoader`, `clientAction`, `shouldRevalidate`, `Layout`, and `HydrateFallback` on root and route modules are now recognized. ([#84](https://github.com/fallow-rs/fallow/pull/84))
- **Gatsby `config` page export and `src/api` function conventions** -- page-level `config` export (Gatsby 5 Slice API) and `src/api/**` serverless function exports (`default`, `config`) are now covered. ([#84](https://github.com/fallow-rs/fallow/pull/84))
- **VitePress `docs/` scaffold discovery** -- `.vitepress` added to hidden directory allowlist, and `docs/.vitepress/config` and `docs/.vitepress/theme/index` patterns added for the common docs-subdirectory layout. Theme entry `default` export is kept alive. ([#84](https://github.com/fallow-rs/fallow/pull/84))

## [2.21.0] - 2026-04-08

### Added

- **Istanbul match-rate stats** -- `istanbul_matched` and `istanbul_total` fields in JSON `summary` when using Istanbul coverage model. Human output footer shows function match count. Helps detect path mismatches or naming gaps in coverage data.
- **Coverage model in snapshots** -- `VitalSignsSnapshot` now persists `coverage_model` (schema v3). Trend output warns with a yellow note when comparing snapshots with different CRAP models, preventing misattribution of score changes.
- **CI auto-detection note** -- when `CI=true` is set and coverage data is auto-detected (not explicit `--coverage`), emits a note on stderr suggesting explicit `--coverage` for deterministic scores.
- **`--coverage-root` flag** -- rebases file paths in Istanbul coverage data by stripping a prefix and prepending the project root. Enables cross-environment coverage matching (CI runner paths vs local checkout). Also available as `coverage_root` MCP parameter.

## [2.20.0] - 2026-04-08

### Added

- **Tiered CRAP scoring with graph-based estimation** -- CRAP scores now use a per-function spectrum instead of per-file binary. Functions whose exports are directly referenced by tests get 85% estimated coverage, indirectly test-reachable functions get 40%, untested files get 0%. The real CRAP formula (CC^2 * (1-cov/100)^3 + CC) is applied with these estimates. Zero config, zero extra latency.
- **Istanbul coverage integration** -- new `--coverage <PATH>` flag (or `FALLOW_COVERAGE` env var) reads `coverage-final.json` from Jest, Vitest, c8, nyc for exact per-function CRAP scores. Auto-detects coverage data from `coverage/coverage-final.json` and `.nyc_output/coverage-final.json`.
- **`oxc_coverage_instrument` dependency** -- uses the published fallow-rs/oxc-coverage-instrument crate for parsing Istanbul-format coverage data.

### Changed

- **Default CRAP model is now `static_estimated`** -- replaces the binary `static_binary` model. The `coverage_model` field in JSON output reflects which model was used: `"static_estimated"` (default) or `"istanbul"` (when coverage data is available).
- **Updated `--explain` CRAP description** -- now documents the tiered estimation model and upgrade path to Istanbul coverage.
- **Updated human output footer** -- file scores footer now describes the estimation model and mentions `--coverage` for exact scores.

## [2.19.3] - 2026-04-08

### Fixed

- **`ignorePatterns` now correctly filters files during discovery** -- user-configured relative glob patterns (e.g. `src/api/generated/**`) were matched against absolute walker paths, so only patterns with `**/` prefix worked. Now strips the project root before matching, consistent with all other glob filters. ([#83](https://github.com/fallow-rs/fallow/issues/83))

## [2.19.2] - 2026-04-08

### Fixed

- **Angular implicit member reference detection** -- `@Component`/`@Directive` metadata extraction now covers inline `template:` strings, `host:` binding expressions, `inputs:`/`outputs:` arrays, and `queries:` metadata. Angular 17+ signal APIs (`input()`, `input.required()`, `output()`, `model()`, `viewChild()`, `viewChildren()`, `contentChild()`, `contentChildren()`) on Angular-decorated classes are treated as framework-managed, preventing false `unused-class-members` reports. ([#82](https://github.com/fallow-rs/fallow/issues/82))

### Changed

- **Module splits for maintainability** -- split `graph/build.rs` narrowing helpers into `graph/narrowing.rs` and `analyze/predicates.rs` into `predicates/{file,import,lifecycle}.rs` for better cohesion. No API changes.
- **Test coverage improvements** -- added 258 tests across 12 files, improving overall line coverage from 90.2% to 90.9%.

## [2.19.1] - 2026-04-07

### Fixed

- **Value/type declaration merging no longer flagged as `duplicate-export`** -- TypeScript allows a const and a type to share the same name (`export const Status = z.enum([...]); export type Status = z.infer<typeof Status>`). This idiomatic pattern (common with Zod, Prisma, and class+interface merging) was falsely reported as a duplicate export. Detection now tracks the type-only flag and skips groups that span both value and type namespaces. ([#81](https://github.com/fallow-rs/fallow/discussions/81))

## [2.19.0] - 2026-04-07

### Added

- **Angular external template tracking** -- `@Component({ templateUrl, styleUrl })` decorators now create graph edges to referenced template and style files, preventing false `unused-files` reports. HTML templates are scanned for Angular syntax (`{{ }}`, `[prop]`, `(event)`, `[(ngModel)]`, `*ngIf`, `*ngFor`, `@if`, `@for`, `@switch`, pipes) and referenced identifiers are bridged to the component's class members, preventing false `unused-class-members` reports. Covers both legacy structural directives and Angular 17+ control flow. ([#73](https://github.com/fallow-rs/fallow/issues/73))

### Fixed

- **Factory initializer instance tracking** -- `const [svc] = useState(() => new MyService())` and similar React hook patterns now correctly resolve the first array-destructured element as a class instance for member access tracking. Prevents false `unused-class-members` when the class is instantiated through a wrapper function. Handles arrow expression bodies, arrow block bodies, and function expressions. ([#79](https://github.com/fallow-rs/fallow/pull/79))
- **VS Code extension binary version skew** -- the auto-downloaded `fallow-lsp` binary in the extension's global storage was never refreshed after initial download, causing LSP diagnostics to diverge from the CLI version. Now checks the installed binary's version on activation and re-downloads when it doesn't match the extension version. ([#80](https://github.com/fallow-rs/fallow/discussions/80))

## [2.18.3] - 2026-04-07

### Fixed

- **`fallow fix` export specifier lists** -- `export { A, B, C }` lines are now fixed correctly by removing individual unused specifiers instead of stripping the `export` keyword (which left invalid `{ A, B, C }` syntax). Handles `export { ... }`, `export { ... } from "..."`, `export type { ... }`, and aliased specifiers. ([#74](https://github.com/fallow-rs/fallow/pull/74))
- **npm publish CI** -- restored two-step npm bootstrap (`npm@10.9.8` then `npm@latest`) for reliable OIDC trusted publishing on Node 22 runners.

## [2.18.2] - 2026-04-07

### Fixed

- **npm publish CI** -- pinned `npm@10` in the release workflow to avoid `promise-retry` module error on Node 22 runners that broke npm package publishing.

## [2.18.1] - 2026-04-07

### Changed

- **Cache serialization switched to bitcode** -- replaced bincode with bitcode for incremental cache serialization, yielding smaller cache files and faster round-trips.
- **Supply chain hardening** -- added OpenSSF Scorecard workflow, zizmor static analysis for GitHub Actions, Dependabot auto-merge for patch/minor dependency bumps, and pinned all CI action SHAs.
- **Miri moved into CI workflow** -- consolidated the standalone Miri workflow into the main CI pipeline for simpler maintenance.

### Fixed

- **Chained member access through class field assignments** -- `this.field.method()` chains where `this.field` is assigned a class instance are now correctly resolved, preventing false positive unused-export reports on the assigned class's methods. ([#63](https://github.com/fallow-rs/fallow/pull/63))
- **Workspace symlink setup idempotent** -- test workspace symlink creation no longer fails when the symlink already exists.

## [2.18.0] - 2026-04-07

### Added

- **`--score`, `--trend`, `--save-snapshot` on combined command** -- the bare `fallow` command (all analyses) now supports health score computation, snapshot saving, and trend comparison. Previously these flags were only available on the `health` subcommand. Enables the health delta header in CI PR/MR comments for combined runs.
- **Health delta header uses metric labels** -- the trend delta line now uses the metric's own label (e.g., "avg complexity" from the trend data) instead of hardcoded text, making the output more robust and descriptive.
- **First-run progressive disclosure** -- when `--score` is enabled but no snapshot exists for trend comparison, the PR comment shows a hint: "Enable `save-snapshot: true` to track score trends over time."
- **Fixture accuracy** -- test fixtures now include `crap_max` and `crap_above_threshold` fields on all `file_scores` entries, matching the actual CLI output schema.

### Fixed

- **GitLab CI `FALLOW_SCORE` and `FALLOW_TREND` wiring** -- both env vars now work for the combined (empty command) case, not just the `health` subcommand.
- **`signed(0)` formatting consistency** -- zero deltas now display as "0.0" instead of "0", consistent with non-zero values like "7.2".
- **GitLab test parity** -- dead export delta assertion (`(-3.8%)`) added to GitLab CI tests, matching the GitHub Action test coverage.
- **CI input descriptions** -- `action.yml` descriptions for `score`, `trend`, and `save-snapshot` now mention bare command support and snapshot storage location.

## [2.17.1] - 2026-04-07

### Added

- **GitHub Action `score` input** -- new `score: true` input enables the `--score` flag on the health command, adding a health delta header to PR summaries showing grade, score change, dead export drift, and complexity trends.
- **Health delta header in CI summaries** -- when `--score` is used, both GitHub Action and GitLab CI summaries display a one-line health score comparison against the previous snapshot.

### Fixed

- **Summary section links** -- all section headers (Code issues, Duplication, Complexity, Codebase health) and table metrics now link to their respective docs pages. Fixes broken markdown link in the Duplication `<summary>` tag.

## [2.17.0] - 2026-04-07

### Added

- **Nuxt runtime convention hardening** -- `plugins/`, `middleware/`, `server/middleware/`, and `components/` directories now have their default exports and named exports (`defineNuxtPlugin`, `defineNuxtRouteMiddleware`, `defineEventHandler`, `defineNuxtComponent`) treated as framework-used. Nested directory scanning for plugins, composables, and utils. Nuxt config `plugins`, `css`, and `modules` entries discovered as runtime entry points with path normalization. ([#60](https://github.com/fallow-rs/fallow/pull/60))
- **Vue SFC edge case hardening** -- `<script src="...">` external script references in Vue SFCs now generate graph edges. `<script>` blocks with `generic="T extends Foo<Bar>"` containing `>` in type parameters are parsed correctly. HTML comments in Vue templates are stripped before import extraction. ([#60](https://github.com/fallow-rs/fallow/pull/60))

### Fixed

- **HTML root-relative path resolution** -- root-relative paths in HTML files (e.g., `<script src="/src/main.tsx">`) are now resolved against the project root instead of being treated as absolute filesystem paths. This fixes false unresolved-import reports in Vite, Parcel, and similar dev server setups. ([#61](https://github.com/fallow-rs/fallow/pull/61))

## [2.16.0] - 2026-04-07

### Added

- **Untested complexity risk (CRAP) scores** -- per-function risk scoring combining cyclomatic complexity with static test reachability from the module graph. Binary model: test-reachable files score CC, untested files score CC^2+CC. Threshold at >=30 (CC>=5 untested), based on the CRAP metric (Savoia & Evans, 2007). Adds `crap_max` and `crap_above_threshold` fields to `FileHealthScore`, `coverage_model: "static_binary"` to `HealthSummary` and `TrendPoint`. Gated behind `--file-scores`. Extreme values (>999) capped in human output; JSON stays uncapped.
- **AddTestCoverage refactoring target** -- new recommendation category (Rule 7) fires when a file has 2+ functions above the CRAP threshold and complexity density > 0.3. Contributing factor `crap_max` appears on targets with high untested risk. Evidence links to the top complex functions.
- **SvelteKit `$types` route group parentheses coverage** -- route groups with parentheses in path segments are now handled correctly for `$types` imports. ([#54](https://github.com/fallow-rs/fallow/pull/54))

### Changed

- **Lazy complexity computation** -- cyclomatic/cognitive complexity is now computed on-demand rather than eagerly, reducing parse time for pipelines that don't need it.
- **Packed ModuleNode flags** -- boolean flags on `ModuleNode` packed into a bitfield for tighter memory layout.
- **Smarter BFS reachability** -- reachability analysis uses optimized graph traversal.
- **Duplicate detection improvements** -- faster clone detection with reduced memory allocation.

### Fixed

- **Windows path separator normalization** -- health coverage-gaps tests now normalize backslashes for Windows CI compatibility.
- **GitHub Action fork PR handling** -- PR comment tests are skipped on fork PRs where write permissions are unavailable.

## [2.15.0] - 2026-04-06

### Added

- **Next.js convention export tracking** -- App Router special files (`loading`, `error`, `not-found`, `template`, `default`, `global-error`, `forbidden`, `unauthorized`, `global-not-found`) and route segment config exports (`revalidate`, `dynamic`, `runtime`, `fetchCache`, `preferredRegion`, `maxDuration`, `viewport`, `generateViewport`) are now treated as framework-used. Pages Router `_app` (`reportWebVitals`), API routes (`config`), `middleware`, `proxy`, `instrumentation` (`register`, `onRequestError`), `instrumentation-client` (`onRouterTransitionStart`), and `mdx-components` (`useMDXComponents`) are also covered. Entry-point files still report genuinely unused helper exports alongside framework-used ones. ([#59](https://github.com/fallow-rs/fallow/pull/59) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Next.js `transpilePackages` support** -- packages listed in `next.config.{ts,js}` `transpilePackages` are treated as referenced dependencies, preventing false unused-dependency reports. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Config-defined alias resolution for Vite, Nuxt, and SvelteKit** -- aliases defined in `vite.config.ts` (`resolve.alias`), `nuxt.config.ts` (`alias`), and `svelte.config.ts` (`kit.alias`) are extracted at analysis time and fed into the resolver as fallbacks. Supports object form, array form, `fileURLToPath(new URL(...))`, `path.resolve(__dirname, ...)`, and `path.join(...)` patterns. Eliminates false unresolved-import, unlisted-dependency, and unused-file reports for config-aliased paths. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Nuxt `srcDir` support** -- when `srcDir` is set in `nuxt.config.ts`, fallow remaps `~/` and `@/` aliases to the configured source directory, adds entry patterns and always-used files under that directory, and resolves `imports.dirs` and `components` paths relative to srcDir. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Nuxt custom directories** -- `imports.dirs`, `components` (string arrays and `{ path: "..." }` objects), and `components.dirs` from `nuxt.config.ts` are now discovered as auto-import roots and component directories. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Nuxt `@/` and `@@/` alias recognition** -- `@/` (srcDir synonym for `~/`) and `@@/` (rootDir synonym for `~~/`) are now recognized as path aliases in both static analysis and resolver fallback. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **SvelteKit param matcher support** -- files in `src/params/**/*.{ts,js}` have their `match` export treated as framework-used. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Vue component tag tracking** -- imported components used as template tags (`<FancyCard />`, `<fancy-card />`, `<Form.Input />`) are now credited as used. Supports PascalCase, kebab-case-to-PascalCase conversion, and namespace member access. Built-in components (`Transition`, `KeepAlive`, `Teleport`, `Suspense`, `Slot`) are excluded. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Vue custom directive tracking** -- custom directives like `v-focus-trap` are mapped to their imported binding name (`vFocusTrap`) and credited as used. Built-in directives (`v-if`, `v-for`, `v-model`, etc.) are excluded. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Vue `v-on` and `v-bind` object syntax** -- `v-on="handlers"` and `v-bind="attrs"` now credit the bound expression as used. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Svelte component tag tracking** -- imported components used as Svelte markup tags (`<FancyButton />`, `<Icons.Alert />`) are credited as used, including namespace member access. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Svelte directive and attribute tracking** -- `use:tooltip`, `transition:fade`, `animate:flip`, `in:fly`, `out:slide` directives credit their imported action/transition binding as used. Attribute value expressions (`class:active={isActive}`) and shorthand attributes (`{page}`) are also tracked. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Svelte `$store` subscription tracking** -- `$page.url.pathname` in Svelte templates now credits the `page` import as used, with `url` tracked as a member access. ([#59](https://github.com/fallow-rs/fallow/pull/59))

### Fixed

- **Svelte template scanner index advancement** -- the HTML tag scanner was advancing by only 1 byte instead of jumping to the end of the scanned tag, causing every HTML tag to be re-parsed from its second byte. ([#59](https://github.com/fallow-rs/fallow/pull/59))
- **Panic on malformed Svelte brace attributes** -- `parse_markup_tag` used `.expect()` on `scan_curly_section` which could panic on unterminated brace expressions in Svelte templates. Replaced with graceful fallback. ([#59](https://github.com/fallow-rs/fallow/pull/59))

## [2.14.2] - 2026-04-06

### Added

- **Svelte/Vue template-visible import tracking** -- imports used only in SFC template markup (e.g., `{formatDate(x)}` in Svelte, `{{ utils.format() }}` in Vue) are now credited as used, preventing false unused-export/import reports. Namespace member access in templates (e.g., `utils.formatDate`) is tracked as member usage. Vue credits only `<script setup>` bindings; Svelte excludes `context="module"` scripts. ([#58](https://github.com/fallow-rs/fallow/pull/58) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Type-only circular dependency filtering** -- `import type` edges are excluded from cycle detection since they are erased at compile time and cannot cause runtime cycles. ([#54](https://github.com/fallow-rs/fallow/issues/54))
- **Duplicate export common-importer filter** -- duplicate exports are only reported when the files sharing the same export name also share a common importer. Unrelated leaf files (e.g., SvelteKit route modules in different directories) are no longer flagged. ([#54](https://github.com/fallow-rs/fallow/issues/54))

### Fixed

- **Workspace plugin merge: generated imports and path aliases** -- `generated_import_patterns` (e.g., SvelteKit `$types`) and `path_aliases` (e.g., SvelteKit `$lib/`) from workspace-level plugins were not propagated to the root analysis, causing false-positive unresolved imports and resolution failures in monorepo setups. ([#54](https://github.com/fallow-rs/fallow/issues/54))
- **Windows path prefix** -- replaced `std::fs::canonicalize()` with `dunce::canonicalize()` to avoid `\\?\` extended-length path prefix on Windows, which broke `oxc_resolver` tsconfig discovery and caused all path-aliased imports to be reported as unresolved. ([#55](https://github.com/fallow-rs/fallow/pull/55) by [@KamilDev](https://github.com/KamilDev))

## [2.14.1] - 2026-04-06

### Added

- **HTML entry file parsing** -- when an HTML file is reachable (e.g., Vite's `index.html` entry point), fallow now parses `<script src>`, `<link rel="stylesheet" href>`, and `<link rel="modulepreload" href>` to create graph edges to referenced local assets. This prevents false-positive dead-code reports for JS/CSS files and their transitive imports in Vite/Parcel-style apps. HTML files are exempt from unused-file detection. ([#57](https://github.com/fallow-rs/fallow/issues/57))
- **Parcel `index.html` entry pattern** -- the Parcel plugin now auto-detects `index.html` as a runtime entry point, matching the Vite plugin's behavior.

### Fixed

- **Coverage gaps inline suppression** -- `coverage-gaps` issue kind can now be suppressed with `// fallow-ignore-next-line coverage-gaps` comments.

## [2.14.0] - 2026-04-06

### Added

- **Static test coverage gaps** (`fallow health --coverage-gaps`) -- reports runtime files and exports that no test dependency path reaches through the module graph. Splits entry points into runtime, test, and support roles across 84+ framework plugins. All 6 output formats supported. ([#53](https://github.com/fallow-rs/fallow/pull/53) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Coverage gaps severity control** -- `coverage-gaps` rule in config (`error`/`warn`/`off`, default `off`). When set to `error`, non-zero exit on any gap. When `warn`, reports gaps without failing CI.
- **Coverage gap actions** -- JSON output includes `add-tests` and `add-test-import` actions for AI agents on untested files and exports.
- **Entry point roles for external plugins** -- `entryPointRole` field (`runtime`/`test`/`support`) on external plugin definitions, allowing custom frameworks to declare how their entry points affect coverage reachability.

### Fixed

- **Workspace entry point fallback scoping** -- `discover_workspace_entry_points` fallback now correctly scopes to the workspace root instead of searching the entire project.
- **External plugin default role** -- external plugin `entryPointRole` now defaults to `support` (matching unknown builtins) instead of `runtime`.

## [2.13.4] - 2026-04-06

### Fixed

- **False positive unused exports from namespace exports** -- `export namespace Foo { export function bar() {} }` no longer reports inner declarations (`bar`) as unused top-level exports. Inner exports are now tracked as namespace members. Runtime namespaces (no `declare`) are correctly classified as non-type-only. ([#52](https://github.com/fallow-rs/fallow/issues/52))

## [2.13.3] - 2026-04-05

### Changed

- **Human output readability** -- all abbreviations spelled out in user-facing output: "deps" → "dependencies", "MI" → "maintainability", "dep" → "dependency". Affects health vital signs, dead-code summary footer, combined orientation header, markdown tables, and score deductions.
- **Section headers in dead-code output** -- human format now groups findings under `── Unused Code ──`, `── Dependencies ──`, and `── Structure ──` headers for faster scanning.
- **Labeled metrics and deductions** -- health score deductions line now prefixed with "Deductions:", metrics lines prefixed with "Metrics:" across health, combined, and audit commands.

### Fixed

- **Boundaries excluded from default `fallow list`** -- `fallow list` no longer shows "Boundaries: not configured" noise; use `--boundaries` to inspect zones and rules. ([#49](https://github.com/fallow-rs/fallow/pull/49) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Ecosystem runner error handling** -- `install_deps()` no longer swallows failures via `|| true`, and stderr is separated from JSON output files. Also uses canonical `dead-code` command with `--quiet`. ([#48](https://github.com/fallow-rs/fallow/pull/48) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Audit vital signs labeled** -- the audit command's vital signs line now includes a "Metrics:" prefix, consistent with health and combined commands.
- **Stale `fallow check` in scripts** -- replaced legacy command name in `bench-ci.sh` and `conformance/run.sh`.

## [2.13.2] - 2026-04-05

### Fixed

- **Plugin entry points included in `--entry-points` mode** -- `fallow list --entry-points` now includes plugin-discovered entry points, matching the behavior of the default `fallow list` output. Previously, plugin detection was skipped when `--entry-points` was used without `--plugins`. ([#45](https://github.com/fallow-rs/fallow/pull/45) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **`Failed` summary line uses canonical command name** -- the summary line printed on analysis failure now says `fallow dead-code` instead of the legacy `fallow check`.
- **Documentation `cargo run` examples** -- all `cargo run` examples in CONTRIBUTING.md and CLAUDE.md now include `--bin fallow` (required for multi-binary workspaces) and use `dead-code` instead of the legacy `check` command. ([#44](https://github.com/fallow-rs/fallow/pull/44) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))

## [2.13.1] - 2026-04-05

### Fixed

- **Init hook uses canonical command name** -- `fallow init --hooks` now generates hooks with `fallow dead-code` instead of the legacy `fallow check` alias, and the internal `--base` field is renamed to `--branch` to match the CLI flag. ([#43](https://github.com/fallow-rs/fallow/pull/43) by [@M-Hassan-Raza](https://github.com/M-Hassan-Raza))
- **Legacy command name cleanup** -- replaced `fallow check` with `fallow dead-code` in 4 user-facing messages: combined output suggestion, GitHub Action PR review body, VS Code extension diagram, and conformance test script.
- **Documentation consistency** -- fixed stale `--base` references to `--branch` in `AGENTS.md`, `docs/backwards-compatibility.md`, and companion repos.

## [2.13.0] - 2026-04-04

### Added

- **Bun built-in module support** -- `bun:sqlite`, `bun:test`, `bun:ffi`, and other `bun:` prefixed imports are now recognized as platform builtins and never flagged as unlisted dependencies. ([#40](https://github.com/fallow-rs/fallow/issues/40))
- **`ignoreDependencies` suppresses unlisted warnings** -- dependencies listed in `ignoreDependencies` are now excluded from both unused dependency AND unlisted dependency detection. Useful for runtime-provided packages like `bun:sqlite` or globally available dependencies. ([#40](https://github.com/fallow-rs/fallow/issues/40))
- **MCP server distributed via npm** -- `fallow-mcp` binary is now included in the npm package. After `npm install fallow`, the `fallow-mcp` command is available for MCP server integration with Claude, OpenCode, and other AI agents. ([#42](https://github.com/fallow-rs/fallow/issues/42))

### Fixed

- **`$schema` accepted in `.fallowrc.json`** -- the JSON schema now includes `$schema` as a valid property, so JSON editors no longer show "unknown key" warnings when using the schema reference. ([#39](https://github.com/fallow-rs/fallow/issues/39))
- **VS Code extension LSP download on Windows** -- the release workflow now names LSP binaries with the correct platform identifier (e.g., `fallow-lsp-win32-x64-msvc.exe`), matching what the VS Code extension expects. Previously, Windows users saw "no LSP binary found" errors. ([#38](https://github.com/fallow-rs/fallow/issues/38))

## [2.12.1] - 2026-04-04

### Fixed

- **Tab indentation preserved in export auto-fix** -- `fallow fix` no longer silently converts tab indentation to spaces when removing the `export` keyword. The original whitespace prefix is now preserved exactly. ([#36](https://github.com/fallow-rs/fallow/issues/36), [#37](https://github.com/fallow-rs/fallow/pull/37) by [@swalha1999](https://github.com/swalha1999))

## [2.12.0] - 2026-04-03

### Added

- **Vital signs percentage referents** -- the orientation header now shows denominators: `dead files 0.6% (1 of 173)` instead of just `0.6%`. Raw counts are also exposed in the health JSON `vital_signs.counts` object for CI dashboards.
- **Entry-point detection inline** -- combined and standalone check modes display `130 entry points detected (124 plugin, 6 package.json)` on stderr, with a yellow warning when zero entry points are found. Entry-point summary is also in the check JSON `entry_points` object.
- **Baseline-aware deltas** -- when `--baseline` is active, the `Failed:` summary line shows `+N since baseline` or `-N since baseline`. A `baseline_deltas` object with per-category deltas is added to the check JSON output.
- **`--summary` flag** -- global flag that shows only category counts without individual items. Works across check (severity-colored counts), dupes (families, groups, lines, rate), and health (functions analyzed, MI, score). JSON output always includes the full `summary` counts object regardless of this flag.
- **`--effort` filter** -- `fallow health --effort low|medium|high` filters refactoring targets by estimated effort level.
- **`--group-by package`** -- groups dead-code findings by workspace package in monorepos. Discovers workspaces automatically from `package.json`/`pnpm-workspace.yaml`.
- **`publicPackages` config** -- workspace package name patterns (exact or glob) whose exports are treated as public API and excluded from unused-export detection.
- **`dynamicallyLoaded` config** -- glob patterns for files loaded at runtime (plugin directories, locale files) that are treated as always-used entry points.
- **`fixture_glob_patterns()` Plugin trait method** -- plugins can now declare test fixture patterns that are implicitly used. Added for Jest, Vitest, and Playwright.
- **Cross-package circular dependency flag** -- `CircularDependency.is_cross_package` indicates cycles crossing workspace boundaries. Shown as `(cross-package)` in human output, present in JSON/SARIF/compact/markdown/CodeClimate.
- **Mirrored directories in JSON** -- `DuplicationReport.mirrored_directories` array with `dir_a`, `dir_b`, `shared_files`, and `total_lines` for CI consumption.
- **Smarter `fallow init`** -- detects project structure (TypeScript, monorepo tool, test framework, UI framework, Storybook) and generates a tailored `.fallowrc.json` with workspace patterns, ignore rules, and entry points.
- **Undeclared workspace diagnostic** -- warns when directories with `package.json` exist but aren't declared in workspace patterns.
- **Second-level directory rollup** -- when one directory holds >80% of unused files, the rollup automatically shows per-subdirectory breakdown (e.g., `packages/svelte/ 4463 files` instead of just `packages/ 4463`).
- **Test-only dependencies in CI summaries** -- `test_only_dependencies` category now appears in GitHub Action and GitLab CI PR summary comments.

### Changed

- **MI legend gated** -- the `MI scale: good ≥85, moderate ≥65, low <65 (0–100)` legend only appears when the average MI is below 85 (moderate or low). Projects with good health scores no longer see it.
- **Config quality note threshold** -- raised from 50% to 80%, reducing noise for projects with moderate test directory concentration.
- **Plugin discovery hint** -- unresolved imports footer now mentions framework plugins: "Framework-specific imports may need a plugin".
- **Scale-aware start-here nudge** -- when total issues exceed 500, the nudge suggests `--workspace <name>` instead of pointing to a specific file.
- **Summary footer filtered counts** -- the `✗ N files · N exports` summary line now reflects visible counts after export suppression, not raw totals.
- **Advisory note stream consistency** -- all advisory notes now consistently use stderr.
- **Rollup suppress hint** -- directory rollup sections suggest `ignorePatterns` in config instead of inline `fallow-ignore-next-line` comments.

### Fixed

- **Unlisted deps false positives** -- shell variables (`$DIR`), pure numbers (`1`), and bundler-internal specifiers (`__barrel_optimize__?...`) are no longer classified as npm package names. Reduces next.js false positives from 761 to 753.
- **Start-here noise filter** -- the "start with X" recommendation no longer points to test fixtures, playground files, or generated files (e.g., `a0.js`). When all targets are noise, the nudge is omitted entirely.
- **Nested node_modules exclusion** -- workspace glob expansion now skips directories inside `node_modules`, preventing third-party package.json files from being analyzed as workspace packages.
- **Init config serialization** -- changed `unwrap_or_else` with silent `{}` fallback to `expect()` for infallible JSON serialization.

## [2.11.0] - 2026-04-03

### Added

- **`--group-by owner|directory`** -- new global flag that groups all dead-code analysis output by team ownership (CODEOWNERS) or by first directory component. CODEOWNERS parser with auto-probe (`.github/CODEOWNERS`, `.gitlab/CODEOWNERS`, `docs/CODEOWNERS`), last-match-wins pattern matching, first-owner-on-line selection. All 6 output formats supported: human (colored group headers with summary line, per-type breakdown, matching rule annotations), JSON (grouped envelope with `groups` array), compact (group prefix per line), markdown (section headers per group), SARIF (`properties.owner`), CodeClimate (`owner` field).
- **`codeowners` config field** -- optional path to a non-standard CODEOWNERS file in `.fallowrc.json` / `fallow.toml`.
- **MCP `group_by` parameter** -- the `analyze` tool now accepts `group_by: "owner" | "directory"` to produce grouped JSON output.

### Changed

- **GitHub Action review comments** -- now filtered to PR diff hunks using `--slurpfile` for large PRs, preventing `ARG_MAX` crashes on PRs with 50+ changed files.
- **GitHub Action review UX** -- improved hunk filtering to tolerate whitespace-only and context lines, with better fallback behavior when no hunk matches.

## [2.10.1] - 2026-04-03

### Changed

- **PR comment vital signs clarity** -- the metrics table in GitHub Action and GitLab CI PR comments now has a "Codebase health" section header, making it clear that metrics are project-wide rather than PR-scoped. The `dead_export_pct` metric is removed from PR comments (still available in CLI JSON output) since it's a graph-level property not actionable from a single PR.
- **Scoped maintainability row** -- when `--changed-since` is active, PR comments now show a "Maintainability (changed files)" row alongside the codebase-wide score, so developers can see how their changes compare to the project baseline.
- **Clickable commit hash** -- the "Scoped to files changed since" footer now links the commit hash to the commit on GitHub/GitLab instead of showing plain text.
- **Disambiguated scoping footer** -- the footer now reads "Issue counts scoped to files changed since ... · health metrics reflect the full codebase" to eliminate ambiguity about what is PR-scoped vs codebase-wide.

## [2.10.0] - 2026-04-03

### Added

- **HTTPS URL extends** -- the `extends` field in `.fallowrc.json` now supports `https://` URLs alongside relative paths and `npm:` packages. Fetch remote shared configs without publishing an npm package. HTTPS-only, 5s default timeout (configurable via `FALLOW_EXTENDS_TIMEOUT_SECS`), 1 MB body limit, URL normalization for cycle detection. URL-sourced configs may extend other URLs or `npm:` packages but not relative paths.
- **GitHub Action shallow clone resilience** -- PR comment, review, annotation, and summary scripts now detect shallow clones and fall back gracefully instead of failing.

## [2.9.3] - 2026-04-03

### Fixed

- **GitHub Action Marketplace** -- shortened action description to meet the 125-character limit required for Marketplace publication.

## [2.9.2] - 2026-04-02

### Added

- **npm package resolution for `extends`** -- config files can now extend shared configs from npm packages using the `npm:` prefix (e.g., `"extends": "npm:@company/fallow-config"`). Resolution walks up `node_modules/`, checks `package.json` `exports`/`main`, and falls back to standard config file names. Subpaths supported (e.g., `npm:@co/config/strict.json`).
- **MCP server hardening** -- improved parameter validation, tool descriptions, and error messages for better AI agent integration.

### Internal

- Path confinement (`resolve_confined`) prevents traversal attacks via npm subpaths or malicious `package.json` exports/main fields.
- Package name validation rejects `..`/`.` components and bare `@scope` without name.

## [2.9.1] - 2026-04-02

### Fixed

- **Deterministic output ordering** -- all result arrays are now sorted after parallel collection, ensuring identical JSON/SARIF/CodeClimate output across runs. Previously, rayon parallelism caused item ordering to vary between invocations.
- **Symlink canonical index warning** -- downgraded noisy symlink resolution warning to debug level.
- **GitHub Action scoping** -- PR comments and annotations now correctly scope to changed files when `--changed-since` is active.

### Changed

- **Performance** -- `OutputFormat`, `MemberKind`, and `DupesMode` enums now derive `Copy`, eliminating unnecessary clones and reference indirection across 20 CLI files.

### Internal

- Split 5 large modules into focused submodules: `regression.rs` (5 modules), `diagnostics.rs` (4 modules), `health_types.rs` (5 modules), plus 3 monolith test files into 30 modules.
- Extracted helper functions from 4 long CLI command functions (1,001 → 365 lines).
- Added 103 unit tests to the config crate (287 → 390 tests).
- Added module-level documentation to all `graph/resolve/` submodules.

## [2.9.0] - 2026-04-01

### Added

- **Architecture boundary violations** -- define zones (glob patterns mapping directories to architecture layers) and rules (which zones may import from which). Violations are detected at the import site using the resolved target's zone. Reported in all 6 output formats, with inline and file-level suppression via `// fallow-ignore-next-line boundary-violation`. LSP diagnostics, code actions, GitHub Action annotations, GitLab CI review comments, and MCP server integration all included.
- **Built-in boundary presets** -- `"preset": "layered"` (4 zones: presentation/application/domain/infrastructure), `"preset": "hexagonal"` (3 zones: adapters/ports/domain), `"preset": "feature-sliced"` (6 zones: app/pages/widgets/features/entities/shared), `"preset": "bulletproof"` (4 zones: app/features/shared/server). Presets auto-detect `rootDir` from `tsconfig.json` for pattern prefixes. User zones and rules merge on top of presets.
- **`fallow list --boundaries`** -- inspect expanded boundary zones, rules, and per-zone file counts. Supports human and JSON output formats.
- **`--boundary-violations` filter** -- show only boundary violations in `fallow dead-code` output.

### Fixed

- **Rest patterns in destructured exports** -- `export const { a, ...rest } = obj` no longer causes a parser error.

### Changed

- **oxc dependency upgrade** -- bumped all 7 oxc crates to latest versions.

## [2.8.1] - 2026-04-01

### Fixed

- **`fallow init` no longer panics** -- the global `--base` alias for `--changed-since` collided with Init's own `--base` flag, causing a runtime panic on every `fallow init` invocation. Init's flag is now `--branch`.

### Added

- **CLI integration test infrastructure** -- shared test harness for all CLI commands with 58 new tests covering `check`, `health`, `dupes`, `init`, exit codes, baselines, and MCP end-to-end. 7 new test fixtures (astro, mdx, complexity, config-file, config-toml, hidden-dir-allowlist, error-no-package-json). Human output snapshots for check, health, and dupes commands.

## [2.8.0] - 2026-04-01

### Added

- **`fallow audit` command** -- combined dead-code + complexity + duplication analysis scoped to changed files, returning a verdict (pass/warn/fail). Purpose-built for reviewing AI-generated code and PR quality gates. Auto-detects the base branch if `--base` is not specified. JSON output includes `verdict`, per-category `summary`, and full sub-results with `actions` arrays. All 6 output formats supported. MCP `audit` tool wraps the CLI.
- **`--base` global alias** -- `--base` is now a global alias for `--changed-since` on all commands. More intuitive for PR review workflows.
- **`.fallow/` added to `.gitignore` during `fallow init`** -- prevents snapshot and cache directories from being committed.

### Changed

- **Release binary uses `panic=abort`** -- smaller binary size by removing unwind tables.
- **Rust 2024 formatting style** -- `.rustfmt.toml` with `style_edition = "2024"`.
- **Stricter lint discipline** -- 6 restriction lints added, `#[expect]` required for all lint suppressions, `tail_expr_drop_order` compiler lint enabled.

## [2.7.3] - 2026-03-31

### Fixed

- **`--format badge` now auto-enables `--score`** -- previously `fallow health --complexity --format badge` would error because score computation wasn't triggered when explicit section flags were passed. Badge format now implies `--score`, matching the behavior of `--min-score`, `--trend`, and `--save-snapshot`.

## [2.7.2] - 2026-03-31

### Added

- **Badge generation** (`--format badge`) -- generates a self-contained shields.io-compatible flat SVG badge showing the health score and letter grade. Grade-first message format (`B (76)`) matches quality dashboard conventions. Color-coded by grade: A=brightgreen, B=green, C=yellow, D=orange, F=red. Unique SVG element IDs prevent collisions when multiple badges are inlined on one page. Also available via `FALLOW_FORMAT=badge` environment variable.

## [2.7.1] - 2026-03-30

### Fixed

- **SvelteKit `$app` and `$env` no longer reported as unlisted dependencies** -- virtual module prefix matching failed when the extracted package name (e.g., `$app`) was compared against a prefix with a trailing slash (`$app/`). Also fixes the same latent bug for Docusaurus virtual prefixes (`@theme/`, `@docusaurus/`, etc.)
- **SvelteKit `./$types` imports no longer reported as unresolved** -- added `generated_import_patterns()` to the Plugin trait so frameworks can declare build-time generated import suffixes. SvelteKit uses this to suppress `./$types`, `./$types.js`, and `./$types.ts` route type imports that are generated at build time and don't exist on disk during static analysis.

## [2.7.0] - 2026-03-30

### Added

- **Structured fix suggestions in JSON output** -- every issue in `--format json` output now includes an `actions` array with machine-actionable fix and suppress hints. 14 fix action types (kebab-case), `auto_fixable` flag on every action, optional `note` for non-auto-fixable items. Dependency issues use `add-to-config` suppress with concrete package name. Re-export findings include a warning note about public API surface. `duplicate_exports` suppress includes `scope: "per-location"`. No schema version bump (additive change).
- **Pre-commit hook setup** (`fallow init --hooks`) -- scaffolds a git pre-commit hook that runs `fallow check --changed-since` on changed files. Auto-detects base branch via git. Supports `--base <ref>` override. Detects husky, lefthook, and bare `.git/hooks`. Includes binary guard and helpful success message with `--no-verify` bypass. Input validated to prevent shell injection.

## [2.6.0] - 2026-03-30

### Added

- **Trend reporting** (`fallow health --trend`) -- compare current health metrics against the most recent saved snapshot and show per-metric deltas with directional indicators (improving/declining/stable). Tracks 8 metrics: health score, dead files, dead exports, avg cyclomatic complexity, maintainability, unused deps, circular deps, and hotspots. Implies `--score`. Reads from `.fallow/snapshots/` (saved via `--save-snapshot`). Output in all formats: human (colored arrows with all-stable collapse), JSON (structured `health_trend` object with raw counts), compact (grep-friendly `trend:overall:direction=` + per-metric lines), and markdown (table for PR comments). Warns when combined with `--changed-since`. Wired through MCP server and GitHub Action.

## [2.5.5] - 2026-03-28

### Fixed

- **Stale cache caused type-level enum detection to not take effect** -- bumped cache version to invalidate entries from v2.5.3/v2.5.4 that were missing the new type-level `whole_object_uses` extraction data

## [2.5.4] - 2026-03-28

### Fixed

- **`Record<Enum, T>` now marks all enum members as used** -- `Record<Status, string>` and nested variants like `Partial<Record<Status, number>>` are now recognized as whole-object enum usage, preventing false unused member reports
- **`keyof typeof Enum` in mapped types now marks all members as used** -- `{ [K in keyof typeof Direction]: string }` is now detected as a whole-object use pattern
- **Relaxed `@types/` unlisted dependency check** -- the type-only import check for `@types/<package>` now works regardless of the import's `type` modifier style

## [2.5.3] - 2026-03-28

### Fixed

- **Type-only imports with `@types/` no longer flagged as unlisted** -- `import type { Feature } from 'geojson'` is no longer reported as an unlisted dependency when `@types/geojson` is in devDependencies. TypeScript erases type-only imports at compile time, so the bare package doesn't need to be installed. Scoped packages are handled via the DefinitelyTyped convention (`@scope/pkg` → `@types/scope__pkg`). Value imports still require the real package.
- **Unused enum member false positives in type-level usage** -- enum members used via TypeScript qualified names in type position (e.g., `type X = Status.Active`) are now detected as used. Enums used as mapped type constraints (e.g., `{ [K in Direction]: string }`) mark all members as used.

## [2.5.2] - 2026-03-28

### Fixed

- **Vitest reporter false positives** -- packages referenced as strings in `test.reporters` (e.g., `vitest-sonar-reporter`) are now detected as used dependencies
- **Vitest coverage/typecheck/browser false positives** -- `test.coverage.provider`, `test.typecheck.checker`, and `test.browser.provider` values are now resolved to their corresponding packages
- **ESLint import resolver false positives** -- packages referenced via `settings["import/resolver"]` (e.g., `eslint-import-resolver-typescript`) are now detected in string, array, and object key formats
- **CI pipeline dependency false positives** -- binaries invoked via `npx` in `.gitlab-ci.yml` and `.github/workflows/*.yml` are now detected as used dependencies

## [2.5.1] - 2026-03-28

### Fixed

- **Absolute paths in duplication suggestions** -- refactoring suggestions (e.g. "Extract ... into src/hooks") were printing absolute filesystem paths instead of project-relative paths

## [2.5.0] - 2026-03-28

### Added

- **Project health score** (`--score`, `--min-score`) -- a single 0-100 number with letter grade (A/B/C/D/F) aggregated from dead code, complexity, maintainability, hotspots, unused dependencies, and circular dependencies. Penalty breakdown in JSON is reproducible: `100 - sum(penalties) == score`. The score is shown by default in `fallow health` output and automatically included in vital signs snapshots (schema v2).
- **CI quality gate** (`--min-score N`) -- exit code 1 when the health score drops below a threshold. Pairs with `--score` for a simple "is this codebase healthy enough?" CI check.
- **Regression detection** (`--fail-on-regression`, `--tolerance`) -- CI gate that compares current issue counts against a previously saved baseline. Supports both absolute (`"5"`) and percentage (`"2%"`) tolerance. Baselines can be saved to a file (`--save-regression-baseline`) or embedded in config.

### Changed

- Health score is included by default when running `fallow health` (no section flags). Use `--score` as a section filter to show only the score.
- `--save-snapshot` now automatically includes the health score and grade in the snapshot (snapshot schema v2). Old v1 snapshots remain readable.

## [2.4.0] - 2026-03-27

### Added

- **Test-only production dependency detection** -- new issue type (14th) flags production dependencies that are only imported by test files, suggesting they be moved to `devDependencies`. Full pipeline: all 6 output formats, LSP diagnostics, baseline support, inline suppression, severity rules (default: `warn`).
- **`tap` test runner support** -- `tap` and `@tapjs/*` packages recognized as tooling dependencies
- **`type-only-dependency` inline suppression** -- `// fallow-ignore-next-line type-only-dependency` now works (was a pre-existing gap where the suppression comment was silently ignored)

### Fixed

- **Tooling over-exemption** -- removed blanket `@babel/`, `babel-`, and `@rollup/` prefix exemptions from unused dependency detection. These packages are now verified against actual config files via plugin parsing, catching ~8 previously missed unused dev dependencies.
- **ESLint shared config following** -- ESLint plugin now reads imported config packages one level deep to discover peer dependencies referenced by shared configs
- **Prettier config parsing** -- new PrettierPlugin extracts plugin references from `.prettierrc` and `prettier.config.*` files
- **Storybook addon object form** -- Storybook plugin now handles `{ name: '@storybook/addon-essentials' }` object entries alongside string entries in the `addons` array

## [2.3.1] - 2026-03-27

### Added

- **Comprehensive test coverage** — 1,200+ new tests across all crates bringing total to 4,700+ unit/integration tests, 101 snapshot tests, 30 property-based tests, 14 doc tests, and 7 conformance fixtures, achieving 91% line coverage
- **CJS `module.exports.foo` detection** — individual property assignments like `module.exports.foo = fn` are now extracted as named exports, closing ~60 missed findings on CJS-heavy projects
- **Conformance test harness** — `tests/conformance/verify-fixtures.sh` and `verify-expected.py` provide automated expected-output verification for 7 analysis scenarios (barrel resolution, circular deps, suppression, type-only imports, and more)

### Fixed

- **Unreachable module export blindspot** — modules not reachable from entry points but containing a mix of used/unused exports were previously skipped entirely; now each export is evaluated individually
- **Dead test file removed** — `unused_exports_tests.rs` (924 lines) was never compiled due to a missing module directive and contained a type mismatch; inline tests already covered all cases

## [2.3.0] - 2026-03-27

### Added

- **GitLab CI rich MR comments** — new `FALLOW_COMMENT` and `FALLOW_REVIEW` variables enable rich MR summary comments with collapsible sections and inline review discussions with suggestion blocks, matching the GitHub Action's review quality
- **GitLab inline review discussions** — posts findings as positioned `DiffNote` discussions on MR diffs with "Why this matters" sections, actionable fix steps, docs links, and one-click suppress instructions
- **GitLab suggestion blocks** — unused export findings include `suggestion:-0+0` blocks for one-click `export` keyword removal directly in the MR diff
- **Comment merging pipeline** — groups unused exports per file into single comments, deduplicates clone group findings, drops redundant refactoring targets, and merges same-line findings with numbered headers
- **Auto `--changed-since` in GitLab MR context** — automatically scopes analysis to changed files using `CI_MERGE_REQUEST_DIFF_BASE_SHA` when running in merge request pipelines
- **Package manager detection** — review comments and annotations now show correct install/uninstall commands (`npm uninstall`, `pnpm remove`, or `yarn remove`) based on lock file detection
- **GitLab comment cleanup** — automatically removes previous fallow comments and discussions on re-runs to prevent comment spam

### Changed

- **GitLab CI template modularized** — inline jq scripts extracted to separate files in `ci/jq/` and `ci/scripts/`, downloaded at runtime for maintainability
- **`diff_refs` from MR API** — GitLab inline discussions now fetch `base_sha`, `start_sha`, `head_sha` from the MR API instead of CI environment variables, matching the ictu-mcp pattern and improving positioning accuracy after rebases
- **Suggestion block ANSI-C quoting** — GitHub Action suggestion blocks fixed to use `$'...'` quoting for correct newline rendering

### Fixed

- **Subshell variable loss** — review comment counters (`POSTED`/`SKIPPED`) now use process substitution instead of pipe subshell to correctly track posting results
- **Comment body assignment** — `comment.sh` separates jq execution from string concatenation to correctly detect jq failures
- **Combined mode null arrays** — `jq -s 'add'` replaced with explicit `jq -n --argjson` to prevent null output when all comment arrays are empty

## [2.2.3] - 2026-03-27

### Added

- **Auto-changed-since for PRs** — GitHub Action automatically scopes analysis to changed files in pull requests using `--changed-since` with the merge base, eliminating the need for manual configuration
- **Enriched PR annotations** — inline annotations now include actionable context (export names, dependency names, file paths) with improved formatting for duplication annotations
- **VS Code status bar and tree views** — expanded extension UX with project health status bar, issue tree view, and pnpm workspace support
- **`analyze_with_parse_result` API** — new public function in `fallow-core` that accepts pre-parsed modules, enabling callers to skip the parsing stage when modules are already available

### Changed

- **Health pipeline optimization** — `fallow health --file-scores` no longer runs the analysis pipeline twice; pre-parsed modules are reused via the new `analyze_with_parse_result` API
- **O(1) tooling dependency lookups** — `GENERAL_TOOLING_EXACT` (76 entries) converted from linear slice scan to `OnceLock<FxHashSet>` for constant-time lookups
- **O(1) unused import binding lookups** — `ResolvedModule.unused_import_bindings` converted from `Vec<String>` to `FxHashSet<String>` in hot-path reference population
- **Optimized member export referencing** — `mark_member_exports_referenced` now uses `FxHashSet<&str>` and avoids per-export `to_string()` allocation
- **Report dispatcher unified** — new `ReportContext` struct replaces individual parameters across all 3 report dispatch functions for consistent signatures
- **`define_plugin!` macro extended** — supports `resolve_config: imports_only` variant; Cypress, Commitlint, Remark plugins migrated
- **Comprehensive code deduplication** — `emit_json()`, `plural()`, `build_json_envelope()`, shared `sample_results` test helper, fix module helpers, config parser shared traversal

### Fixed

- **Watch mode reload stability** — hardened debounce behavior and related cleanup
- **Windows CI path normalization** — discovery tests now normalize path separators for cross-platform compatibility
- **GitHub Action `pull_request_target` handling** — correctly detects and handles `pull_request_target` events in auto-changed-since logic

### Removed

- **1,986 lines of dead code** — removed orphaned `crates/graph/src/graph/build/` directory that was never compiled (abandoned refactoring artifact)

## [2.2.2] - 2026-03-27

### Added

- **CodeClimate output format** — `--format codeclimate` for GitLab Code Quality integration, with deterministic FNV-1a fingerprints and proper severity mapping
- **GitHub Actions inline annotations** — `--format annotations` emits `::warning` / `::error` workflow commands for inline PR annotations without any Action dependency
- **Real-world conformance benchmarks** — CI now validates against 8 real-world projects (zod, preact, vite, next.js, angular, nuxt, svelte, vue-core)
- **~283 new tests** — comprehensive coverage for complexity metrics, JSDoc @public tags, config extends/merge, re-export chain propagation, dynamic import patterns, declaration extraction, visitor helpers, analysis predicates, cycle detection, and file discovery

### Fixed

- **CodeClimate fingerprint stability** — use FNV-1a instead of `DefaultHasher` for deterministic cross-run fingerprints; include group index in duplication fingerprints
- **Circular dependency annotations** — sanitize chain strings and guard against empty files in annotation output
- **npm/pnpm install stdout leak** — suppress package manager install stdout that leaked into JSON report output
- **Duplicate exports comparison** — handle dict locations correctly in `duplicate_exports` comparison

## [2.2.1] - 2026-03-26

### Changed

- **Parallel workspace processing** — workspace entry point discovery and plugin runs now execute in parallel using rayon, with sequential merge for deterministic results. Up to 21% faster on monorepos (vite: 507ms → 399ms, next.js: 1532ms → 1371ms)
- **Lazy canonicalize** — skips upfront bulk `canonicalize()` of all source files when the project root is already canonical (common case). A `OnceLock`-based fallback handles the rare intra-project symlink edge case on demand. Saves up to 148ms on 20k-file projects
- **O(1) plugin dedup** — workspace plugin name and virtual module prefix deduplication uses `FxHashSet` instead of `Vec::contains` (O(n²) → O(n))

### Fixed

- **Benchmark accuracy** — benchmark script now correctly excludes knip runs that crash (exit code 2) instead of counting crash timings as valid results. Also guards against null status from timeouts
- **Updated benchmark numbers** — rebenchmarked all projects with honest error handling. Speed claims updated: 5-41x vs knip v5 (was 3-36x), 2-18x vs knip v6 (was 2-14x), 8-26x vs jscpd (was 20-33x)

## [2.2.0] - 2026-03-26

### Added

- **Efficiency score** — refactoring targets now include an `efficiency` field (`priority / effort`) and are sorted by efficiency descending, surfacing quick wins first
- **Confidence levels** — each target includes a `confidence` field (`high`/`medium`/`low`) based on data source reliability: `high` for graph/AST analysis, `medium` for heuristic thresholds, `low` for git-dependent recommendations
- **Adaptive thresholds** — fan-in/fan-out normalization uses percentile-based thresholds (p95/p90/p75/p25) from the project's distribution instead of hardcoded constants, with floors to prevent degenerate values in small projects
- **Target thresholds in JSON** — `target_thresholds` object in health JSON output exposes the computed adaptive thresholds for programmatic consumers
- **Effort summary** — human output shows effort breakdown after the targets header (e.g., `16 low effort · 34 medium · 43 high`)
- **Machine-parseable compact categories** — compact output uses underscore-delimited category labels (`circular_dep`, `dead_code`) instead of space-separated labels

### Changed

- **Human output: efficiency as primary score** — the hero number is now efficiency (sort key), with priority shown as a dimmed secondary value
- **Human output: labeled metadata** — effort and confidence on line 2 are now prefixed (`effort:low · confidence:high`) for self-documenting output
- **Markdown table: 5 columns** — reduced from 7 to 5 columns by merging effort/confidence and dropping the separate priority column
- **SARIF messages** — now include priority, efficiency, and confidence values

### Fixed

- **Cycle path deduplication** — `evidence.cycle_path` no longer contains duplicate entries when a file participates in multiple cycles
- **GitLab CI template** — uses Alpine image and detects package manager correctly
- **Benchmark alert threshold** — corrected for `customBiggerIsBetter` benchmarks
- **SARIF version redaction** in test fixtures
- **MCP analyze tool description** — corrected to match `dead-code` command

## [2.1.0] - 2026-03-25

### Added

- **GitLab CI template** (`ci/gitlab-ci.yml`) — includable template with full feature parity to the GitHub Action: Code Quality reports (CodeClimate format) for inline MR annotations, MR comment summaries, incremental caching, and all fallow commands/options via `FALLOW_*` variables
- **GitHub Action: test workflow** — CI validation for SARIF, JSON, dupes, fix, zero-issues, and PR comment scenarios

### Fixed

- **`list --no-cache`** — the `--no-cache` flag now works correctly with the `list` command
- **GitHub Action: `check` → `dead-code` rename** — completed the rename across all case statements, SARIF fallback, and job summary dispatch
- **`dead-code` subcommand** in backwards-compatibility stable interface list

## [2.0.1] - 2026-03-25

### Added

- **MCP server: all global CLI flags exposed** — `--baseline`, `--save-baseline`, `--no-cache`, `--threads` now available on all MCP tools; `--config` gap-filled on `find_dupes`/`check_health`; `--workspace` gap-filled on `find_dupes`/`fix_preview`/`fix_apply`
- **GitHub Action: 13 new inputs** — `no-cache`, `threads`, `only`, `skip`, `cross-language`, `file-scores`, `hotspots`, `targets`, `complexity`, `since`, `min-commits`, `save-snapshot`, `issue-types`
- **GitHub Action: `dead-code` alias support** — all case statements now handle both `dead-code` and legacy `check` command names
- **GitHub Action: bare invocation support** — combined issue count extraction, job summary, and PR comments work when no command is specified

### Fixed

- **GitHub Action: `fix` without `--dry-run` now adds `--yes`** — previously would hang in CI waiting for TTY input

## [2.0.0] - 2026-03-25

### Breaking

- **Bare `fallow` now runs all analyses** (check + dupes + health). Previously it ran only dead code analysis. Use `fallow dead-code` or `fallow --only check` for the old behavior.
- **`check` renamed to `dead-code`**: `fallow dead-code` is now the canonical name for dead code analysis. `fallow check` remains as a hidden alias for backwards compatibility.
- **`--ci`, `--fail-on-issues`, `--sarif-file` are now global flags**: work with all commands (check, dupes, health, bare). Previously check-only.
- **Combined JSON output**: bare `fallow --format json` produces `{ "check": {...}, "dupes": {...}, "health": {...} }` instead of a flat check-only object.
- **Combined SARIF output**: bare `fallow --format sarif` produces a multi-run SARIF document with one run per analysis.
- **MCP server tools** now use `dead-code` subcommand internally.

### Added

- **Combined analysis command**: bare `fallow` runs dead code + duplication + complexity in a single invocation with combined output across all 5 formats
- **`--only` / `--skip` flags**: select which analyses to run when using bare `fallow` (e.g., `fallow --only check,dupes`, `fallow --skip health`)
- **`dead-code` command alias**: `fallow dead-code` as the canonical name for dead code analysis (replaces `check`)
- **`--ci` on all commands**: `fallow dupes --ci` and `fallow health --ci` now work (SARIF + quiet + fail-on-issues)
- **Vital signs snapshots** (`fallow health --save-snapshot`): save codebase health metrics for trend tracking
- **Execute/run split**: internal refactor enabling combined mode — `execute_check`, `execute_dupes`, `execute_health` return results without printing

### Changed

- Codebase refactored: 10 large files split into focused submodules for maintainability

## [1.9.0] - 2026-03-25

### Added
- **Refactoring targets** (`fallow health --targets`): ranked, actionable recommendations that synthesize complexity, coupling, churn, and dead code signals into a prioritized list of files to refactor
  - Seven recommendation rules evaluated in priority order: urgent churn+complexity, break circular dependency, split high-impact file, remove dead code, extract complex functions, reduce coupling
  - Priority formula: `min(density,1)×30 + hotspot×25 + dead_code×20 + fan_in_norm×15 + fan_out_norm×10`
  - Contributing factors with raw `value` and `threshold` for programmatic use
  - **Effort estimation** (`low`/`medium`/`high`) based on file size, function count, and fan-in — shown in all output formats
  - **Evidence linking**: structured data for AI agents — unused export names, complex function names with line numbers, cycle member paths
  - **Baseline support**: `--save-baseline` / `--baseline` now includes refactoring targets for tracking progress over time
  - All five output formats: human (category · effort labels), JSON (with evidence), compact, markdown (Effort column), SARIF (warning-level findings)
  - MCP server: `targets` parameter on `check_health` tool
- VS Code extension overhaul: LSP notifications, walkthrough, diagnostics improvements, tree view enhancements
- Clickable documentation links in all diagnostics + JSON schema for config
- ~100 unit tests across 6 crates

### Fixed
- 11 bug fixes across parser, analysis, baseline, LSP, and performance
- Empty tree views now show "no issues found" after analysis
- `hasAnalyzed` context set from CLI analysis to clear welcome views
- Removed buggy extract duplicate code action
- Qualified `Span` in benchmark to resolve ambiguous import

## [1.8.1] - 2026-03-25

### Added
- LSP pull-model diagnostics (`textDocument/diagnostic`) for Cursor/VSCodium compatibility
- Multi-root workspace support in LSP: discovers monorepo workspaces via `package.json` workspaces, pnpm-workspace.yaml, and tsconfig references
- Precise diagnostic ranges for unresolved imports: squiggly underline now covers only the module specifier string literal, not the entire import line
- `source_span` field on `ImportInfo` for precise source string literal positions

### Fixed
- LSP `textDocument/diagnostic` errors in Cursor (Method not found -32601)
- Unused dependency diagnostics now appear on the correct line in package.json (was always line 1)
- Circular dependency diagnostics now appear on the import statement that starts the cycle
- File-level diagnostics (unused files, unlisted deps) highlight the entire first line instead of a zero-width marker
- Stale cached diagnostics cleared when issues are resolved between analysis runs
- Extract cache version bumped to 16 for new `ImportInfo.source_span` field

### Changed
- Internal refactoring: split large modules (human.rs, visitor.rs, resolve.rs, registry.rs, etc.) into focused submodules
- Improved test coverage across graph, extract, core, and LSP crates

## [1.8.0] - 2026-03-24

### Added
- **Human output polish**: completely redesigned terminal output across all three commands for readability at scale
  - Per-section footers with one-line explanations and docs links (always shown, replaces `--explain` for human output)
  - Mirrored directory detection in dupes: collapses `src/ ↔ deno/lib/` patterns into a single group
  - Circular dependency hub grouping: cycles sharing the same file are grouped with path elision
  - Duplicate exports stacked vertically with path elision for long monorepo paths
  - Global truncation: all sections default to 10 items with `... and N more` overflow hints
  - Thousands separators for large numbers (e.g., `5,433` lines)
- **`--complexity` flag** for `fallow health`: select only the complexity findings section
- **Health defaults to all sections**: `fallow health` now shows complexity + file-scores + hotspots by default. Use `--complexity`, `--file-scores`, or `--hotspots` to select specific sections.
- **`--explain` flag**: adds `_meta` object with metric definitions and docs URLs to JSON output. Human output always includes explanations.
- **`--top` flag** for `fallow dupes`: limit the number of clone groups shown
- `elide_common_prefix` utility for shorter paths in dependency chains and duplicate exports

### Changed
- Summary footer uses shortened labels (`27 files · 89 exports` instead of `27 unused files · 89 unused exports`)
- Health footer compacted to single line (`✗ 22 above threshold · 3111 analyzed · MI 90.5`)
- Dupes footer uses `✗` prefix consistent with check and health
- Health complexity findings use two-line format (function name on line 1, metrics on line 2)
- Single-group clone families suppressed from default output
- Tree connectors (├─ └─) replaced with simple indentation in dupes
- All docs URLs updated to `/explanations/{dead-code,health,duplication}` with section anchors
- Config warnings use `tracing::warn!` instead of `eprintln!`
- CI workflow permissions scoped to per-job level

## [1.7.0] - 2026-03-24

### Added
- Hotspot analysis (`fallow health --hotspots`): combines git churn history with complexity data to surface the riskiest files in a codebase. Score formula: `normalized_churn × normalized_complexity × 100` (0-100 scale). Recency-weighted commit count with exponential decay (half-life 90 days). Trend detection labels files as accelerating, stable, or cooling.
  - `--since` accepts durations (`6m`, `90d`, `1y`, `2w`) and ISO dates (`2025-06-01`), default 6 months
  - `--min-commits` threshold (default 3) excludes low-activity files from ranking
  - Fan-in shown as separate "blast radius" column
  - Shallow clone detection with warning
  - Available in all output formats (human, JSON, compact, markdown)
  - MCP `check_health` tool supports `hotspots`, `since`, and `min_commits` parameters

### Changed
- Health human output uses two-line format for hotspots and file scores: score/MI on first line with path (dimmed directory, bold filename), metrics on indented second line
- Renamed "lines" to "churn" in hotspot output across all formats to avoid ambiguity with file length

### Fixed
- MCP tests on Windows: skip `/bin/sh`-dependent tests on non-Unix platforms
- Typo checker false positive and Windows path separator in list tests

## [1.6.1] - 2026-03-24

### Added
- Per-file health scores (`fallow health --file-scores`): maintainability index combining complexity density, dead code ratio, fan-in, and fan-out. Available in all output formats (human, JSON, compact, markdown, SARIF). MCP `check_health` tool supports `file_scores: true` parameter.
- Markdown and SARIF output formats for health command
- Workspace scoping (`--workspace`) and baseline support (`--baseline`/`--save-baseline`) for health command

### Fixed
- Re-export chain propagation for entry-point-only exports: exports consumed solely via re-export from an entry point barrel (e.g., `export { render } from './render'` in `src/index.js`) are no longer falsely reported as unused. Fixes false positives for the common library barrel pattern.
- Entry point star re-exports (`export * from './source'`) now correctly exclude the default export per ES spec
- Deterministic bare specifier resolution: removed the `BareSpecifierCache` that caused non-deterministic results in multi-threaded mode when per-file tsconfig path aliases resolved the same specifier to different targets. Replaced with a deterministic post-resolution pass that upgrades `NpmPackage` to `InternalModule`. Analysis results are now identical between `--threads 1` and default multi-threaded mode.
- Hard error on health baseline I/O failures instead of silent fallback
- MCP server test and review findings addressed

## [1.6.0] - 2026-03-24

### Added
- Precise line/column locations in SARIF output for all issue types that previously defaulted to line 1
  - Unlisted dependencies: SARIF now points to the actual import statement in the source file
  - Unused dependencies (all variants): SARIF now points to the dependency entry line in package.json
  - Type-only dependencies: SARIF now points to the dependency entry line in package.json
  - Circular dependencies: SARIF now points to the import statement that starts the cycle
- `ImportSite` type in JSON output for unlisted dependencies (replaces plain path strings with `{path, line, col}` objects)
- `find_import_span_start()` method on `ModuleGraph` for looking up import statement locations between modules

### Changed
- JSON output `schema_version` bumped to 3 (new fields on `UnusedDependency`, `TypeOnlyDependency`, `CircularDependency`)
- `UnlistedDependency.imported_from` changed from `string[]` to `ImportSite[]` in JSON output
- `UnusedDependency` now includes `line` field (1-based line in package.json)
- `TypeOnlyDependency` now includes `line` field (1-based line in package.json)
- `CircularDependency` now includes `line` and `col` fields (location of the cycle-starting import)

### Fixed
- GitHub Code Scanning alerts for unlisted/unused dependencies no longer point to `package.json:1`
- GitHub Code Scanning alerts for circular dependencies no longer point to line 1 of the first file
- `output-schema.json` `DuplicateExport.locations` corrected from `string[]` to proper `DuplicateLocation[]` objects
- `output-schema.json` now includes missing `unused_optional_dependencies` field

## [1.5.0] - 2026-03-24

### Added
- `fallow health` command: per-function cyclomatic complexity (McCabe classic) and cognitive complexity (SonarSource algorithm) analysis with configurable thresholds (default: cyclomatic 20, cognitive 15), `--top N`, `--sort`, `--changed-since`, and human/JSON/compact output formats
- `[health]` config section with `maxCyclomatic`, `maxCognitive`, and `ignore` glob patterns
- `check_health` MCP tool for AI agent integration
- Function name capture from class methods, object method shorthand, variable declarators, property definitions, and `export default function()`

### Changed
- Duplicate export locations now include line and column numbers (JSON output `schema_version` bumped to 2)
- GitHub Action job summary redesigned with tables, icons, and collapsible sections
- Cache version bumped to 15 (old caches auto-invalidate)

### Fixed
- SARIF URIs now percent-encode brackets for Next.js dynamic routes (`[slug]` → `%5Bslug%5D`)
- JSON output uses relative paths for readable CI summaries
- GHAS availability check before SARIF upload in GitHub Action

## [1.4.0] - 2026-03-23

### Added
- Class instance member tracking: `const svc = new MyService(); svc.greet()` correctly tracks `greet` as a used class member, reducing false positives for class-based code (NestJS services, Angular components, etc.)
- JSDoc `@public` tag support: exports annotated with `/** @public */` (or `/** @api public */`) are never reported as unused, designed for library authors whose exports are consumed by external projects
- Type-only dependencies rule: production dependencies only imported via `import type` are reported as `type-only-dependency` (they should be devDependencies since types are erased at runtime)
- Progress spinners showing analysis pipeline stages (discovery, parsing, resolution, analysis) for better UX on larger projects
- VS Code extension published on Open VSX for Cursor and VSCodium support
- Historical performance metric tracking with GitHub Pages dashboard
- Package.json `#subpath` imports integration test

### Changed
- VS Code extension bundler migrated from esbuild to Rolldown
- Large modules split into focused submodules for maintainability: `discover.rs`, `config.rs`, `resolve.rs`, `detect.rs`, `cache.rs`, `visitor.rs`, `check.rs`, and integration tests
- Watch mode now shows changed file paths and clears screen between runs

### Fixed
- SARIF output now includes `tool.driver.version` and `$schema` fields
- Conformance workflow JSON output handling

## [1.3.1] - 2026-03-23

### Added
- `dupes --changed-since`: duplication detection now supports `--changed-since` to only report clone groups involving changed files
- Angular plugin `resolve_config()`: parses `angular.json` to extract `styles`, `scripts`, `main`, `browser`, and `polyfills` from build targets as entry points; adds Angular peer dependencies (`rxjs`, `@angular/common`, `@angular/platform-browser`, `@angular/build`) to tooling deps; widens entry patterns for Nx monorepo layouts
- Nx plugin `resolve_config()`: parses `project.json` to extract executor references as dependencies and `options.main` as entry points; adds `@nx/angular`, `@nx/storybook`, `@nx/webpack`, and other `@nx/*` packages to tooling deps
- JSON config file filesystem fallback: plugins with JSON config patterns (e.g., `angular.json`, `**/project.json`) are now discovered via filesystem scan when they're not in the JS/TS discovered file set; workspace-level plugins also check the project root for config files
- File-based plugin activation for ESLint and Vitest: plugins now activate when their config files exist in a workspace, not just when the package is in `dependencies`. Fixes false positives in monorepos where `eslint`/`vitest` are only in the root `package.json`
- Vitest plugin marks `setupTests.{ts,tsx,js,jsx}` and `test-setup.{ts,tsx,js,jsx}` as always-used when active, fixing false positives for test setup files referenced via imported/spread base configs
- Nested package entry discovery now searches `services/`, `tools/`, and `utils/` directories in addition to `packages/`, `apps/`, `libs/`, `modules/`, `plugins/`
- Infrastructure entry point detection: Dockerfiles, Procfiles, and `fly.toml` are scanned for source file references

### Fixed
- Bare specifier cache poisoning: the resolver cache for bare specifiers (e.g., `@scope/pkg`) now only caches results from successful `oxc_resolver` resolution; previously, when resolution failed for a tsconfig path alias that looked like an npm package, the `NpmPackage` fallback was cached and prevented correct resolution for all subsequent files
- Production dependency false positives: `plugin_tooling` dependencies (e.g., `zone.js`, `@angular/compiler`) are now excluded from unused production dependency detection, matching the existing dev dependency behavior
- Workspace entry pattern double-prefixing: entry patterns from `resolve_config()` that are already project-root-relative (e.g., `apps/client/src/styles.css` from `angular.json`) are no longer double-prefixed with the workspace path
- `check --changed-since` now filters all issue types: unlisted dependencies, duplicate exports, and circular dependencies were previously unfiltered
- Duplication stats recomputation: `recompute_stats` now deduplicates overlapping line ranges per file (matching the original `compute_stats` logic), fixing inflated `duplicated_lines` counts after baseline or `--changed-since` filtering
- Plugin entry point attribution: entry points discovered by plugins now show the correct plugin name (e.g., `Plugin { name: "nextjs" }`) instead of the generic `Plugin { name: "plugin" }`

## [1.3.0] - 2026-03-22

### Added
- Unused optionalDependencies detection: packages in `optionalDependencies` never imported or used as script binaries are now reported
- Function overload deduplication: multiple overload signatures for the same function name are deduplicated to avoid false positive unused-export reports

### Changed
- Workspace discovery performance: canonicalize optimization reduces syscall overhead during workspace resolution
- Re-export chain performance: `matches_str` optimization for faster re-export chain propagation

### Fixed
- Ecosystem test script: `--root` flag and install commands corrected for cross-project validation

## [1.2.0] - 2026-03-22

### Added
- Unused import binding detection via `oxc_semantic`: imports where the bound name is never read in the importing file no longer count as references to the exported symbol, improving unused-export detection precision
- Namespace destructuring detection: `const { foo, bar } = ns` after `import * as ns` (and dynamic import / require namespaces) now correctly tracks accessed members for narrowing

### Fixed
- Namespace imports with whole-object consumption patterns (`Object.values(ns)`, `{...ns}`, `for..in ns`, `const { a, ...rest } = ns`) now correctly mark all exports as referenced instead of being skipped by the entry-point guard

## [1.1.0] - 2026-03-21

### Added
- Markdown output format (`--format markdown`)
- Oxc-inspired code quality: workspace-level clippy configuration with `all`, `pedantic`, `nursery`, `cargo` lint groups and 13 restriction lints
- `#[expect]` over `#[allow]` for all clippy suppressions (warns when suppression becomes unnecessary)
- Const size assertions on hot-path types (`ModuleNode`, `ModuleInfo`, `ExportInfo`, `ImportInfo`, `Edge`) to prevent accidental struct bloat
- VS Code extension icon and marketplace metadata

### Changed
- Module graph split into focused submodules (`types.rs`, `build.rs`, `reachability.rs`, `re_exports.rs`, `cycles.rs`)
- Dev profile optimized with `debug = false` and selective `opt-level` for proc-macro crates

### Fixed
- Windows build: restrict `ModuleNode` size assertion to Unix (`PathBuf` layout differs on Windows MSVC)

## [1.0.4] - 2026-03-21

### Fixed
- CI failures across typos, cargo-deny, docs, MSRV, and clippy
- Windows symlink support for workspace integration test

## [1.0.3] - 2026-03-20

### Fixed
- npm publish: switch to OIDC trusted publishing (no NPM_TOKEN secret needed)

## [1.0.2] - 2026-03-20

### Fixed
- npm publish: add registry authentication to release workflow

## [1.0.1] - 2026-03-20

### Fixed
- Windows build: restrict `DiscoveredFile` size assertion to unix (different `PathBuf` layout on Windows MSVC)

## [1.0.0] - 2026-03-20

### Added
- TypeScript project references: workspace discovery from `tsconfig.json` `references` field (additive with npm/pnpm workspaces, deduplicated by canonical path)
- Elementary cycle enumeration for circular dependencies (individual cycles within SCCs, max 20 per SCC, deterministic output)
- CSS Modules support (`.module.css`/`.module.scss`) with class name export tracking
- `fallow migrate` command to convert knip and jscpd configs
- CSS/SCSS file discovery with `@import`, `@use`, `@forward`, and `@apply` extraction
- Cross-workspace `exports` field subpath resolution with output→source fallback
- Pnpm content-addressable store detection for injected dependencies
- Cache-aware incremental parsing with `--performance` cache hit/miss stats
- Code Lens with export reference counts and click-to-navigate in LSP
- Duplication diagnostics and "Extract duplicate into function" code action in LSP
- VS Code extension CI, LSP binary builds, and marketplace publishing
- Nuxt `resolve_config()` for deep config parsing (modules, css, plugins, extends, postcss, path aliases)
- Circular dependency benchmarks vs madge and dpdm
- Inline suppression for circular dependencies (`fallow-ignore-file circular-dependency`)
- Backwards compatibility policy (`docs/backwards-compatibility.md`)
- JSON output schema documentation (`docs/output-schema.json`)

### Fixed
- UTF-8 boundary handling in duplication detection (multi-byte character safety)
- Exports map resolution robustness for cross-workspace imports
- Nested output subdirectory mapping (e.g., `dist/esm/utils.mjs` → `src/utils.ts`)
- Trace path matching for monorepo compatibility (canonicalized vs user-provided paths)

## [0.3.0] - 2026-03-18

### Added
- Production mode (`--production`) — excludes test/dev files, limits to production scripts, reports type-only imports
- Clone families with refactoring suggestions (extract function/module)
- Config schema generation (`fallow config-schema`) with `$schema` support for IDE autocomplete
- Duplication baselines (`--save-baseline` / `--baseline`) for incremental CI adoption
- Deep `resolve_config()` for all top 10 framework plugins (ESLint, Vite, Jest, Storybook, Tailwind, Webpack, TypeScript, Babel, Rollup, PostCSS)
- JSONC as default config format (with JSON and TOML still supported)
- Pre-commit hooks for fmt and clippy checks

### Changed
- Migrated repository to fallow-rs organization

### Fixed
- Detection accuracy: star re-export tracking, workspace plugins, JSX fallback
- GitHub Action YAML parse error and benchmark workflow

### Performance
- Optimized resolve, discovery, and duplication detection hotpaths

## [0.2.0] - 2026-03-18

### Added
- Code duplication detection (`fallow dupes`) with suffix array + LCP algorithm
- 4 detection modes: strict, mild, weak, semantic
- Duplication benchmarks comparing fallow dupes vs jscpd (4-75x faster)
- Plugin system with 40 built-in framework plugins as single source of truth
- Dynamic import resolution (template literals, `import.meta.glob`, `require.context`)
- Vue/Svelte SFC parsing with `<script>` block extraction
- Dynamic member heuristics (Object.values/keys/entries, for..in, spread)
- GitHub Action with job summaries, PR comments, and `--sarif-file` flag
- LSP diagnostics for all 10 issue types

### Fixed
- CJS require analysis and namespace narrowing for detection accuracy

### Performance
- Clone detection pipeline optimized up to 177x faster vs initial implementation

## [0.1.7] - 2026-03-17

### Added
- Roadmap and project documentation

### Changed
- Improved human report formatting

## [0.1.6] - 2026-03-17

### Fixed
- Reduced false positives based on knip comparison across 6 public projects

## [0.1.5] - 2026-03-17

### Added
- Plugin system, baseline comparison, and `fallow fix` command
- Comprehensive test coverage for high-priority gaps
- CI regression tracking benchmarks

### Fixed
- Reduced false positives based on knip comparison across 31 projects
- Byte column counting, strip_prefix, and HashSet reuse
- Deduplicated imported_from paths in unlisted dependency reports

## [0.1.4] - 2026-03-17

### Fixed
- Reduced false positives and improved re-export chain detection
- Repository URL casing for GitHub provenance

## [0.1.3] - 2026-03-17

### Fixed
- npm publish path resolution

### Changed
- Updated GitHub Actions to Node.js 24, added npm READMEs

## [0.1.2] - 2026-03-17

### Fixed
- Repository URLs and npm publish error surfacing

## [0.1.1] - 2026-03-17

### Added
- Initial public release
- Dead code analysis with 10 issue types
- npm publishing pipeline with platform-specific binaries (macOS, Linux, Windows)
- LSP server with diagnostics and code actions
- CLI commands: check, watch, fix, init, list, schema
- 4 output formats: human, JSON, SARIF, compact
- Rules system with per-issue-type severity
- Inline suppression comments
- `--changed-since` and `--fail-on-issues` for CI
- Cross-workspace resolution for npm/yarn/pnpm workspaces

[Unreleased]: https://github.com/fallow-rs/fallow/compare/v2.76.0...HEAD
[2.76.0]: https://github.com/fallow-rs/fallow/compare/v2.75.0...v2.76.0
[2.75.0]: https://github.com/fallow-rs/fallow/compare/v2.74.0...v2.75.0
[2.74.0]: https://github.com/fallow-rs/fallow/compare/v2.73.0...v2.74.0
[2.73.0]: https://github.com/fallow-rs/fallow/compare/v2.72.0...v2.73.0
[2.72.0]: https://github.com/fallow-rs/fallow/compare/v2.71.1...v2.72.0
[2.71.1]: https://github.com/fallow-rs/fallow/compare/v2.71.0...v2.71.1
[2.71.0]: https://github.com/fallow-rs/fallow/compare/v2.70.0...v2.71.0
[2.70.0]: https://github.com/fallow-rs/fallow/compare/v2.69.0...v2.70.0
[2.69.0]: https://github.com/fallow-rs/fallow/compare/v2.68.0...v2.69.0
[2.68.0]: https://github.com/fallow-rs/fallow/compare/v2.67.0...v2.68.0
[2.67.0]: https://github.com/fallow-rs/fallow/compare/v2.66.2...v2.67.0
[2.66.2]: https://github.com/fallow-rs/fallow/compare/v2.66.1...v2.66.2
[2.66.1]: https://github.com/fallow-rs/fallow/compare/v2.66.0...v2.66.1
[2.66.0]: https://github.com/fallow-rs/fallow/compare/v2.65.0...v2.66.0
[2.65.0]: https://github.com/fallow-rs/fallow/compare/v2.64.0...v2.65.0
[2.64.0]: https://github.com/fallow-rs/fallow/compare/v2.63.0...v2.64.0
[2.63.0]: https://github.com/fallow-rs/fallow/compare/v2.62.0...v2.63.0
[2.62.0]: https://github.com/fallow-rs/fallow/compare/v2.61.0...v2.62.0
[2.61.0]: https://github.com/fallow-rs/fallow/compare/v2.60.0...v2.61.0
[2.60.0]: https://github.com/fallow-rs/fallow/compare/v2.59.0...v2.60.0
[2.59.0]: https://github.com/fallow-rs/fallow/compare/v2.58.0...v2.59.0
[2.58.0]: https://github.com/fallow-rs/fallow/compare/v2.57.0...v2.58.0
[2.57.0]: https://github.com/fallow-rs/fallow/compare/v2.56.0...v2.57.0
[2.56.0]: https://github.com/fallow-rs/fallow/compare/v2.55.0...v2.56.0
[2.55.0]: https://github.com/fallow-rs/fallow/compare/v2.54.3...v2.55.0
[2.54.3]: https://github.com/fallow-rs/fallow/compare/v2.54.2...v2.54.3
[2.54.2]: https://github.com/fallow-rs/fallow/compare/v2.54.1...v2.54.2
[2.54.1]: https://github.com/fallow-rs/fallow/compare/v2.54.0...v2.54.1
[2.54.0]: https://github.com/fallow-rs/fallow/compare/v2.53.0...v2.54.0
[2.53.0]: https://github.com/fallow-rs/fallow/compare/v2.52.2...v2.53.0
[2.52.2]: https://github.com/fallow-rs/fallow/compare/v2.52.1...v2.52.2
[2.52.1]: https://github.com/fallow-rs/fallow/compare/v2.52.0...v2.52.1
[2.52.0]: https://github.com/fallow-rs/fallow/compare/v2.51.0...v2.52.0
[2.51.0]: https://github.com/fallow-rs/fallow/compare/v2.50.0...v2.51.0
[2.50.0]: https://github.com/fallow-rs/fallow/compare/v2.49.0...v2.50.0
[2.49.0]: https://github.com/fallow-rs/fallow/compare/v2.48.5...v2.49.0
[2.48.5]: https://github.com/fallow-rs/fallow/compare/v2.48.4...v2.48.5
[2.48.4]: https://github.com/fallow-rs/fallow/compare/v2.48.3...v2.48.4
[2.48.3]: https://github.com/fallow-rs/fallow/compare/v2.48.2...v2.48.3
[2.48.2]: https://github.com/fallow-rs/fallow/compare/v2.48.1...v2.48.2
[2.48.1]: https://github.com/fallow-rs/fallow/compare/v2.48.0...v2.48.1
[2.48.0]: https://github.com/fallow-rs/fallow/compare/v2.47.1...v2.48.0
[2.47.1]: https://github.com/fallow-rs/fallow/compare/v2.47.0...v2.47.1
[2.47.0]: https://github.com/fallow-rs/fallow/compare/v2.46.0...v2.47.0
[2.46.0]: https://github.com/fallow-rs/fallow/compare/v2.45.1...v2.46.0
[2.45.1]: https://github.com/fallow-rs/fallow/compare/v2.45.0...v2.45.1
[2.45.0]: https://github.com/fallow-rs/fallow/compare/v2.44.2...v2.45.0
[2.44.2]: https://github.com/fallow-rs/fallow/compare/v2.44.1...v2.44.2
[2.44.1]: https://github.com/fallow-rs/fallow/compare/v2.44.0...v2.44.1
[2.44.0]: https://github.com/fallow-rs/fallow/compare/v2.43.0...v2.44.0
[2.43.0]: https://github.com/fallow-rs/fallow/compare/v2.42.0...v2.43.0
[2.42.0]: https://github.com/fallow-rs/fallow/compare/v2.41.0...v2.42.0
[2.41.0]: https://github.com/fallow-rs/fallow/compare/v2.40.3...v2.41.0
[2.40.3]: https://github.com/fallow-rs/fallow/compare/v2.40.2...v2.40.3
[2.40.2]: https://github.com/fallow-rs/fallow/compare/v2.40.1...v2.40.2
[2.40.1]: https://github.com/fallow-rs/fallow/compare/v2.40.0...v2.40.1
[2.40.0]: https://github.com/fallow-rs/fallow/compare/v2.39.0...v2.40.0
[2.39.0]: https://github.com/fallow-rs/fallow/compare/v2.38.0...v2.39.0
[2.38.0]: https://github.com/fallow-rs/fallow/compare/v2.37.0...v2.38.0
[2.37.0]: https://github.com/fallow-rs/fallow/compare/v2.36.0...v2.37.0
[2.36.0]: https://github.com/fallow-rs/fallow/compare/v2.35.0...v2.36.0
[2.35.0]: https://github.com/fallow-rs/fallow/compare/v2.34.0...v2.35.0
[2.34.0]: https://github.com/fallow-rs/fallow/compare/v2.33.0...v2.34.0
[2.33.0]: https://github.com/fallow-rs/fallow/compare/v2.32.1...v2.33.0
[2.32.1]: https://github.com/fallow-rs/fallow/compare/v2.32.0...v2.32.1
[2.32.0]: https://github.com/fallow-rs/fallow/compare/v2.31.0...v2.32.0
[2.31.0]: https://github.com/fallow-rs/fallow/compare/v2.30.0...v2.31.0
[2.30.0]: https://github.com/fallow-rs/fallow/compare/v2.29.1...v2.30.0
[2.29.1]: https://github.com/fallow-rs/fallow/compare/v2.29.0...v2.29.1
[2.29.0]: https://github.com/fallow-rs/fallow/compare/v2.28.2...v2.29.0
[2.28.2]: https://github.com/fallow-rs/fallow/compare/v2.28.1...v2.28.2
[2.28.1]: https://github.com/fallow-rs/fallow/compare/v2.28.0...v2.28.1
[2.28.0]: https://github.com/fallow-rs/fallow/compare/v2.27.6...v2.28.0
[2.27.6]: https://github.com/fallow-rs/fallow/compare/v2.27.5...v2.27.6
[2.27.5]: https://github.com/fallow-rs/fallow/compare/v2.27.4...v2.27.5
[2.27.4]: https://github.com/fallow-rs/fallow/compare/v2.27.3...v2.27.4
[2.27.3]: https://github.com/fallow-rs/fallow/compare/v2.27.2...v2.27.3
[2.27.2]: https://github.com/fallow-rs/fallow/compare/v2.27.1...v2.27.2
[2.27.1]: https://github.com/fallow-rs/fallow/compare/v2.27.0...v2.27.1
[2.27.0]: https://github.com/fallow-rs/fallow/compare/v2.26.1...v2.27.0
[2.26.1]: https://github.com/fallow-rs/fallow/compare/v2.26.0...v2.26.1
[2.26.0]: https://github.com/fallow-rs/fallow/compare/v2.25.1...v2.26.0
[2.25.1]: https://github.com/fallow-rs/fallow/compare/v2.25.0...v2.25.1
[2.25.0]: https://github.com/fallow-rs/fallow/compare/v2.24.0...v2.25.0
[2.24.0]: https://github.com/fallow-rs/fallow/compare/v2.23.1...v2.24.0
[2.23.1]: https://github.com/fallow-rs/fallow/compare/v2.23.0...v2.23.1
[2.23.0]: https://github.com/fallow-rs/fallow/compare/v2.22.4...v2.23.0
[2.22.4]: https://github.com/fallow-rs/fallow/compare/v2.22.3...v2.22.4
[2.22.3]: https://github.com/fallow-rs/fallow/compare/v2.22.2...v2.22.3
[2.22.2]: https://github.com/fallow-rs/fallow/compare/v2.22.1...v2.22.2
[2.22.1]: https://github.com/fallow-rs/fallow/compare/v2.22.0...v2.22.1
[2.22.0]: https://github.com/fallow-rs/fallow/compare/v2.21.0...v2.22.0
[2.21.0]: https://github.com/fallow-rs/fallow/compare/v2.20.0...v2.21.0
[2.20.0]: https://github.com/fallow-rs/fallow/compare/v2.19.3...v2.20.0
[2.19.3]: https://github.com/fallow-rs/fallow/compare/v2.19.2...v2.19.3
[2.19.2]: https://github.com/fallow-rs/fallow/compare/v2.19.1...v2.19.2
[2.19.1]: https://github.com/fallow-rs/fallow/compare/v2.19.0...v2.19.1
[2.19.0]: https://github.com/fallow-rs/fallow/compare/v2.18.3...v2.19.0
[2.18.3]: https://github.com/fallow-rs/fallow/compare/v2.18.2...v2.18.3
[2.18.2]: https://github.com/fallow-rs/fallow/compare/v2.18.1...v2.18.2
[2.18.1]: https://github.com/fallow-rs/fallow/compare/v2.18.0...v2.18.1
[2.18.0]: https://github.com/fallow-rs/fallow/compare/v2.17.1...v2.18.0
[2.17.1]: https://github.com/fallow-rs/fallow/compare/v2.17.0...v2.17.1
[2.17.0]: https://github.com/fallow-rs/fallow/compare/v2.16.0...v2.17.0
[2.16.0]: https://github.com/fallow-rs/fallow/compare/v2.15.0...v2.16.0
[2.15.0]: https://github.com/fallow-rs/fallow/compare/v2.14.2...v2.15.0
[2.14.2]: https://github.com/fallow-rs/fallow/compare/v2.14.1...v2.14.2
[2.14.1]: https://github.com/fallow-rs/fallow/compare/v2.14.0...v2.14.1
[2.14.0]: https://github.com/fallow-rs/fallow/compare/v2.13.4...v2.14.0
[2.13.4]: https://github.com/fallow-rs/fallow/compare/v2.13.3...v2.13.4
[2.13.3]: https://github.com/fallow-rs/fallow/compare/v2.13.2...v2.13.3
[2.13.2]: https://github.com/fallow-rs/fallow/compare/v2.13.1...v2.13.2
[2.13.1]: https://github.com/fallow-rs/fallow/compare/v2.13.0...v2.13.1
[2.13.0]: https://github.com/fallow-rs/fallow/compare/v2.12.1...v2.13.0
[2.12.1]: https://github.com/fallow-rs/fallow/compare/v2.12.0...v2.12.1
[2.12.0]: https://github.com/fallow-rs/fallow/compare/v2.11.0...v2.12.0
[2.11.0]: https://github.com/fallow-rs/fallow/compare/v2.10.1...v2.11.0
[2.10.1]: https://github.com/fallow-rs/fallow/compare/v2.10.0...v2.10.1
[2.10.0]: https://github.com/fallow-rs/fallow/compare/v2.9.3...v2.10.0
[2.9.3]: https://github.com/fallow-rs/fallow/compare/v2.9.2...v2.9.3
[2.9.2]: https://github.com/fallow-rs/fallow/compare/v2.9.1...v2.9.2
[2.9.1]: https://github.com/fallow-rs/fallow/compare/v2.9.0...v2.9.1
[2.9.0]: https://github.com/fallow-rs/fallow/compare/v2.8.1...v2.9.0
[2.8.1]: https://github.com/fallow-rs/fallow/compare/v2.8.0...v2.8.1
[2.8.0]: https://github.com/fallow-rs/fallow/compare/v2.7.3...v2.8.0
[2.7.3]: https://github.com/fallow-rs/fallow/compare/v2.7.2...v2.7.3
[2.7.2]: https://github.com/fallow-rs/fallow/compare/v2.7.1...v2.7.2
[2.7.1]: https://github.com/fallow-rs/fallow/compare/v2.7.0...v2.7.1
[2.7.0]: https://github.com/fallow-rs/fallow/compare/v2.6.0...v2.7.0
[2.6.0]: https://github.com/fallow-rs/fallow/compare/v2.5.5...v2.6.0
[2.5.5]: https://github.com/fallow-rs/fallow/compare/v2.5.4...v2.5.5
[2.5.4]: https://github.com/fallow-rs/fallow/compare/v2.5.3...v2.5.4
[2.5.3]: https://github.com/fallow-rs/fallow/compare/v2.5.2...v2.5.3
[2.5.2]: https://github.com/fallow-rs/fallow/compare/v2.5.1...v2.5.2
[2.5.1]: https://github.com/fallow-rs/fallow/compare/v2.5.0...v2.5.1
[2.5.0]: https://github.com/fallow-rs/fallow/compare/v2.4.0...v2.5.0
[2.4.0]: https://github.com/fallow-rs/fallow/compare/v2.3.1...v2.4.0
[2.3.1]: https://github.com/fallow-rs/fallow/compare/v2.3.0...v2.3.1
[2.3.0]: https://github.com/fallow-rs/fallow/compare/v2.2.3...v2.3.0
[2.2.3]: https://github.com/fallow-rs/fallow/compare/v2.2.1...v2.2.3
[2.2.1]: https://github.com/fallow-rs/fallow/compare/v2.2.0...v2.2.1
[2.2.0]: https://github.com/fallow-rs/fallow/compare/v2.1.0...v2.2.0
[2.1.0]: https://github.com/fallow-rs/fallow/compare/v2.0.1...v2.1.0
[2.0.1]: https://github.com/fallow-rs/fallow/compare/v2.0.0...v2.0.1
[2.0.0]: https://github.com/fallow-rs/fallow/compare/v1.9.0...v2.0.0
[1.9.0]: https://github.com/fallow-rs/fallow/compare/v1.8.1...v1.9.0
[1.8.1]: https://github.com/fallow-rs/fallow/compare/v1.8.0...v1.8.1
[1.8.0]: https://github.com/fallow-rs/fallow/compare/v1.7.0...v1.8.0
[1.7.0]: https://github.com/fallow-rs/fallow/compare/v1.6.1...v1.7.0
[1.6.1]: https://github.com/fallow-rs/fallow/compare/v1.6.0...v1.6.1
[1.6.0]: https://github.com/fallow-rs/fallow/compare/v1.5.0...v1.6.0
[2.1.0]: https://github.com/fallow-rs/fallow/compare/v2.0.1...v2.1.0
[1.5.0]: https://github.com/fallow-rs/fallow/compare/v1.4.0...v1.5.0
[1.4.0]: https://github.com/fallow-rs/fallow/compare/v1.3.1...v1.4.0
[1.3.1]: https://github.com/fallow-rs/fallow/compare/v1.3.0...v1.3.1
[1.3.0]: https://github.com/fallow-rs/fallow/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/fallow-rs/fallow/compare/v1.1.0...v1.2.0
[1.1.0]: https://github.com/fallow-rs/fallow/compare/v1.0.4...v1.1.0
[1.0.4]: https://github.com/fallow-rs/fallow/compare/v1.0.3...v1.0.4
[1.0.3]: https://github.com/fallow-rs/fallow/compare/v1.0.2...v1.0.3
[1.0.2]: https://github.com/fallow-rs/fallow/compare/v1.0.1...v1.0.2
[1.0.1]: https://github.com/fallow-rs/fallow/compare/v1.0.0...v1.0.1
[2.3.0]: https://github.com/fallow-rs/fallow/compare/v2.2.3...v2.3.0
[2.2.3]: https://github.com/fallow-rs/fallow/compare/v2.2.2...v2.2.3
[2.2.2]: https://github.com/fallow-rs/fallow/compare/v2.2.1...v2.2.2
[1.0.0]: https://github.com/fallow-rs/fallow/compare/v0.3.0...v1.0.0
[0.3.0]: https://github.com/fallow-rs/fallow/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/fallow-rs/fallow/compare/v0.1.7...v0.2.0
[0.1.7]: https://github.com/fallow-rs/fallow/compare/v0.1.6...v0.1.7
[0.1.6]: https://github.com/fallow-rs/fallow/compare/v0.1.5...v0.1.6
[0.1.5]: https://github.com/fallow-rs/fallow/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/fallow-rs/fallow/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/fallow-rs/fallow/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/fallow-rs/fallow/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/fallow-rs/fallow/releases/tag/v0.1.1
