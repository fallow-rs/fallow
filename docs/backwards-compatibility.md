# Backwards Compatibility Policy

Starting with v1.0, fallow follows [semantic versioning](https://semver.org/).

## What is stable

These interfaces are covered by semver , breaking changes only happen in major version bumps:

### Configuration format

- **Config file names**: `.fallowrc.json`, `.fallowrc.jsonc`, `fallow.toml`, `.fallow.toml`
- **All documented config fields**: `minimumVersion`, `extends`, `ignorePatterns`, `ignoreFindings`, `rules`, `overrides`, `entry`, `ignoreDependencies`, `ignoreExports`, `ignoreExportsUsedInFile`, `ignoreDecorators`, `unusedComponentProps` (with `ignorePattern`), `includeEntryExports`, `autoImports`, `duplicates`, `audit`, `cache`, `fix`, `production` (boolean form `production: true` or per-analysis form `production: { deadCode, health, dupes }`), `framework`, `workspaces`, `plugins`, `rulePacks`, `boundaries` (including `boundaries.preset`, `boundaries.coverage`, and `boundaries.calls`)
- **Duplication triage fields**: `duplicates.near` enables opt-in function-level near-miss detection. `duplicates.ignoredClones` accepts normalized `<fingerprint>:<instance_count>` keys and resurfaces a reviewed group when its token content or occurrence count changes.
- **Rule names and severity values**: `unused-files`, `unused-exports`, etc. with `error`/`warn`/`off`
- **Extends and overrides semantics**: merge behavior, glob matching, override precedence, `npm:` prefix resolution, `https://` URL resolution
- **`ignoreFindings` semantics**: patterns are validated as project-root-relative globs, leading `!` patterns keep reportable exceptions, source-owned dead-code findings with multiple owners are hidden only when every owner matches, and architecture, policy, suppression-hygiene, and framework-correctness findings remain visible, as do manifest-owned findings that no source file owns (unused dependencies, unused dev and optional dependencies, catalog entries, and dependency overrides). An unlisted dependency is source-owned by its import sites, so it is hidden only when every import site matches. The opt-in React health signals are source-owned too, so they are hidden as well: a thin wrapper by its component file, a duplicate prop shape by the file of the component the finding is emitted for (siblings keep their own findings and still list the hidden component in `sharing_components`), and a prop-drilling chain only when every hop file matches. Security candidates and their blind-spot metadata (`fallow security` findings, unresolved-callee diagnostics, and unresolved-edge counts) are never hidden by `ignoreFindings`: a path glob must not silence a leak candidate or turn an unresolved blind spot into a clean bill
- **Unknown config keys and version skew**: an unrecognized key fails the run with exit 2, so a plausible-but-wrong `ignorePaths` cannot silently do nothing where `ignorePatterns` was meant. That strictness makes a NEW config field a coordinated upgrade: a runner on an older version fails on the commit that adds the field, not on the upgrade that would explain it. `minimumVersion` names the floor a config is written for, as `MAJOR.MINOR.PATCH`. A binary below it stops with a message naming both versions and the upgrade, before reporting any field it does not recognize, so the shared-config case reads as version skew rather than as a typo. It is inherited through `extends`, so a monorepo base config can declare the floor once. At or above the floor an unknown key still fails: there the key really is a typo. The field is optional and changes nothing when absent.
- **Config file syntax**: both `.fallowrc.json` and `.fallowrc.jsonc` are read as JSONC, so comments and trailing commas are accepted in either file. The published config schema (`schema.json`) states that with the `allowComments` and `allowTrailingCommas` keywords the JSON language service defines, so editors validate the same dialect the loader accepts. They are annotations: fallow never reads them back, and a validator running in a strict mode that rejects unknown keywords should be configured to allow them. Loose JSONC extensions (unquoted keys, single-quoted strings, missing commas, hexadecimal numbers, unary plus) stay rejected so config files remain portable to other JSONC tooling.
- **Inline suppression comment syntax**: `fallow-ignore-next-line`, `fallow-ignore-file`

### JSON output schema

- **Whitespace is not part of the JSON contract**: consumers must parse JSON
  rather than compare or split raw text. `--format json` emits compact JSON by
  default, while global `--pretty` selects indented presentation. Both forms
  carry the same values and end with exactly one line feed. This presentation
  choice does not change `schema_version`.

- **Top-level structure**: `schema_version`, `version`, `elapsed_ms`, `total_issues`, and all issue arrays
- **Issue type arrays**: `unused_files`, `unused_exports`, `unused_types`, `private_type_leaks`, `unused_dependencies`, `unused_dev_dependencies`, `unused_enum_members`, `unused_class_members`, `unresolved_imports`, `unlisted_dependencies`, `duplicate_exports`, `type_only_dependencies`, `circular_dependencies`, `re_export_cycles`, `boundary_violations`, `boundary_coverage_violations`, `boundary_call_violations`, `policy_violations`
- **Issue object fields**: all fields documented in `docs/output-schema.json`
- **Volatile fields**: exactly three fields may differ between two runs of the same command over the same commit: `elapsed_ms` (a measured duration), `head_sha` (the base snapshot's own commit, which moves when the working tree commits between runs), and `_meta.telemetry.analysis_run_id` (a per-run identifier by construction). Everything else in a report is reproducible: rerun the same command on the same commit and the remaining bytes are identical, whatever thread count the run used. This is the contract the repository's determinism gates enforce, and `strip_volatile_fields` in `crates/cli/tests/common/mod.rs` is the single helper they strip with: it removes the two fields named in `VOLATILE_REPORT_FIELDS` at any depth, plus the nested `_meta.telemetry.analysis_run_id`. A field that starts moving between runs is a bug, not a new volatile field.
- **Schema version**: each output envelope versions independently from the tool and from sibling envelopes. The affected envelope is bumped when an EXISTING wire field is renamed, removed, or its type changes, when a value is added to an existing enum-valued required field, OR when a `required` field is added to a previously-documented finding. An envelope that embeds the changed contract bumps too; unrelated envelopes do not. Additive optional fields (new fields with `#[serde(skip_serializing_if = ...)]` that are absent on the wire by default, or new finding types added to brand-new issue-type arrays) do NOT bump `schema_version`: existing consumers see a byte-identical wire shape on the unchanged path. Exact envelope versions are encoded as numeric `const` values in `docs/output-schema.json`; a shared CLI/programmatic shape with separate version lineages encodes the closed numeric set. The generated TypeScript contract derives its literal types from those schema definitions. One field is an explicit exception to the enum-value rule above: `workspace_diagnostics[].kind` is an OPEN set. A new diagnostic kind may be added without bumping any envelope, because a diagnostic is advisory, is omitted when the run records none, and never changes an exit code, so a consumer that does not recognise a kind can ignore the entry and lose nothing. `docs/output-schema.json` still enumerates every kind fallow can currently emit, so a consumer validating against a PINNED older copy of the schema will reject an entry carrying a newer kind: validate against the schema shipped with the version you run, and treat an unknown `kind` as "some diagnostic" rather than an error. This is the same tolerate-unknown-values contract `duplication.clone_groups[].demotion_reason` documents. Kinds added under this exception, without a bump, are `bun-lockb-override-resolution-skipped` ([#2358](https://github.com/fallow-rs/fallow/issues/2358)), `malformed-pnpm-workspace-yaml` ([#2148](https://github.com/fallow-rs/fallow/issues/2148)), `skipped-source-dotdir` ([#461](https://github.com/fallow-rs/fallow/issues/461)), `source-parse-degraded`, which reports a source file that was read but did not parse cleanly, carrying `error_count` and `panicked`, `node-modules-missing`, which reports a project with no installed dependency tree (package `exports` and conditional exports cannot be read, plugins that activate on an installed package stay inactive, and dependency classification degrades), `excluded-by-default-ignore` ([#2638](https://github.com/fallow-rs/fallow/issues/2638)), which reports candidate source files one of fallow's built-in discovery ignore patterns removed from the walk, carrying `pattern`, `file_count`, and `directory_count`, and `boundaries-not-configured` / `rule-packs-not-configured`, which report that a detector's zero violation count comes from nothing being configured rather than from nothing being found. Those last two are the unconfigured zero only: a project that sets `boundary-violation` or `policy-violation` to `off` chose that silence and is not reported. That last kind is reported only: a degraded parse never withholds a finding, because oxc also reports recoverable errors for valid syntax newer than the parser, so gating on it would mute real results project-wide.
- **Audit duplication demotion fields**: under `gate: new-only`, an introduced clone group none of whose instances overlap an added line from the run's diff is demoted to inherited (issue #2164). The demotion is observable (issue #2220): the demoted entry in `duplication.clone_groups[]` carries an additive optional `demotion_reason` string (kebab-case, currently `no-added-lines`; further values may be added, so treat unknown values as "some demotion reason"), and audit-family attribution blocks (`fallow audit --format json`, `fallow review --format json`, the MCP `audit` tool) always include an integer `attribution.duplication_demoted` derived from those entries. Demoted groups stay counted in `duplication_inherited`, so `duplication_demoted <= duplication_inherited`. Both fields follow the styling-attribution precedent: additive audit-family JSON, no `schema_version` bump. `fallow dupes --format json` never emits `demotion_reason`.
- **Audit brief branching block**: the `audit-brief` envelope shared by `fallow audit --brief --format json` and `fallow review --format json`, and the brief digest embedded in the review walkthrough guide, may carry an additive optional `branching` object. It holds `split_in_place`, an array naming each changed file present on both revisions whose branch-point total stayed within `tolerance` while it gained functions and its peak fell, with both sides of all three numbers. Test paths, generated and vendored paths, and files carrying synthetic template units are excluded from that array; their totals still count towards the changeset figures below. The array describes a shape and does not assert that a refactor happened, plus the changeset totals (`branch_points`, `functions`, `peak_unit_cyclomatic`, each with `previous`/`current`/`delta`), a `scope` block (`files_both`, `files_added`, `files_only_in_base`, the test-path split and the largest single file's share), the separate `branch_points_only_in_base` total, a `cognitive` block whose `attributed_to` is absent when cognitive did not fall, the published `tolerance`, and a capped `by_file` list with `by_file_omitted`. There is no changeset-level verdict field. The object is absent when the run had no base snapshot to compare against. It is verdict-neutral: it does not participate in the audit verdict, the exit code, or the pull-request decision surface. Additive optional under the rule above, so no `schema_version` moves. Consumers that pin the exact key set of the brief envelope should widen it by one.
- **Audit styling fields**: `fallow audit` includes styling analytics by default. The nested `complexity` block (the health sub-analysis payload) may contain `css_analytics`, `styling_health`, and `styling_findings` for CSS, Sass/Less, CSS Modules, Tailwind/shadcn/CVA, StyleX/PandaCSS, vanilla-extract, styled-components, and Emotion projects. Under `gate: new-only`, styling findings carry the same optional `introduced` marker as other findings and the attribution block includes `styling_introduced` / `styling_inherited` totals. These fields are additive JSON output, and styling findings are verdict-neutral unless the corresponding rule is configured to `error`; they do not require a `schema_version` bump under the additive-field policy. Snapshot-diffing consumers can set `audit.css: false` or pass `--no-css` to suppress styling entirely.
- **Audit brief focus score components**: `focus.review_here[].score.security_taint` and `focus.deprioritized[].score.security_taint` are now omitted from the wire while the component is zero, the same treatment `runtime` already had. It is a permanently zero component today (no security pass is threaded onto the brief path), and publishing it as a required field made it read as a measurement that found nothing rather than as something nothing measured. It reappears with a non-zero value if and when a security pass feeds the brief. A consumer that sums the components must read an ABSENT component as zero; `total` is unchanged and still equals the sum of every component that ran. This REMOVES a wire field on every run rather than adding an optional one, so it is not covered by the additive-field exemption: the `audit-brief` envelope shared by `fallow audit --brief --format json` and `fallow review --format json`, and the brief digest embedded in the review walkthrough guide, bump `schema_version` from 9 to 10. Schema 9 already introduced the impact-closure count and sample changes documented below. Consumers pinning the exact key set of `score` should widen it to treat both `security_taint` and `runtime` as optional.
- **Audit brief ownership section** (2026-09-24): the `audit-brief` envelope shared by `fallow audit --brief --format json` and `fallow review --format json`, and the brief digest embedded in the review walkthrough guide, may carry an additive optional `ownership` object ([#2599](https://github.com/fallow-rs/fallow/issues/2599)). Fallow computes it from the CODEOWNERS file alone and reads no git history. It holds `group_count`, `transitive_only_count`, `unowned_direct_count`, a capped `groups` rollup of `{ owner, direct_count, affected_count }` with `groups_omitted`, and an optional `slices` array. `slices` is present only when `partition.independent_slices` is present, aligned by index, and each entry is `{ module_dirs, owners, separable }`. Each file maps to its primary owner, and a file that no rule owns belongs to the `(unowned)` group. The key is absent when the project has no CODEOWNERS file. The field is additive, so the general policy does not require a version bump. The brief `schema_version` moves from 10 to 11 as a deliberate additive bump for capability detection: without it, a consumer cannot tell "no CODEOWNERS file" from "an older fallow" when the key is absent. Consumers that need the section should require `schema_version >= 11`.
- **Audit and combined workspace diagnostics**: three envelopes report more in `workspace_diagnostics[]`, and a shared dedupe fix widens the list and programmatic envelopes on one project shape (issue #2366). `fallow audit --format json` and the `audit-brief` envelope shared by `fallow review --format json` and `fallow audit --brief --format json` carried the config-load workspace-discovery kinds and the source-discovery kinds under `dead_code.workspace_diagnostics[]`, and now additionally carry the two analysis-stage kinds (`malformed-pnpm-workspace-yaml`, `bun-lockb-override-resolution-skipped`) and `undeclared-workspace`, which the analyze pipeline appends after the config-load stash; a consumer pinning the exact set of kinds those two envelopes can emit should widen it by three, not two. The bare combined `fallow --format json` gained a NEW top-level `workspace_diagnostics[]`, an optional field absent when the run records no diagnostics, which is additive under the rule above; only there was the array previously always absent. The two audit-family paths are the same `CheckOutput` array the standalone `dead-code` envelope already documents. All three use root-relative paths, are deduplicated on the whole `kind` (payload included) plus `path` so two overlapping workspace globs still report the same directory once per pattern, and are omitted when empty; none bumps `schema_version`. The combined envelope's `check`, `dupes`, and `health` sections never carry the array. That payload-aware dedupe is shared, so two further surfaces report one entry more on a project where two workspace globs match the same package-less directory: bare `fallow list --format json`, and every envelope built from an engine session's diagnostics snapshot (the MCP `project_info`, `find_dupes`, and `check_health` tools plus the programmatic dead-code and combined routes). Those previously collapsed the two `glob-matched-no-package-json` entries into one and now agree with `fallow list --workspaces --format json`, which always reported both. Three consequences of the payload deciding identity: the recorded `pattern` drops a no-op `./` prefix (`"./apps/**"` is reported as `apps/**`, in the JSON field and in the warning text; a glob spelled exactly `"./"`, the project root itself, keeps its spelling); the recorded `path` drops the matching no-op `.` component, so one directory has one spelling on every envelope instead of `./pkgs/aaa` on the analysis envelopes next to `pkgs/aaa` on the workspace listing; and workspace discovery deduplicates before it returns, not only the process registry. Because `package.json` `workspaces`, `pnpm-workspace.yaml` `packages`, `deno.json` `workspace`, and the root `tsconfig.json` references are additive sources, a repository declaring one glob in two of them (the conventional pnpm layout) reported every package-less directory under it once per spelling and now reports it once, on `fallow dead-code`, `check`, `dupes`, `health`, `list --workspaces`, and `workspaces --format json`, through the MCP `project_info` tool, and under `dead_code` in `fallow audit` and `fallow review --format json`; the aggregated stderr warning, built from the same list, likewise names the true directory count with each example once, as do the `N workspace discovery diagnostics` summary line every human-format command prints and the per-entry block `fallow workspaces` and `fallow list --workspaces` print. SARIF, markdown, compact, badge, CodeClimate, and the cache format carry no workspace diagnostic and are unchanged. The two shapes are independent: the `./` normalisation alone changes the recorded `pattern`, `path`, and `message` on a repository that declares each glob once through one source, so a consumer pinning snapshots should expect movement whenever a manifest spells a workspace glob with a leading `./`, whether or not that glob is declared twice. The same fold covers a second shape on the same surfaces: a malformed workspace member reached through both an npm glob and a root `tsconfig.json` `references[]` entry reported one `malformed-package-json` diagnostic per source and now reports one in total. Two overlapping globs declared in one manifest still report the same directory once per `pattern`. No kind is new on any of these envelopes and no field changes type, so no `schema_version` moves; a consumer counting entries per directory should expect one per distinct matching pattern.
- **`hotspot_summary` carries the clock its numbers were measured against**: `hotspot_summary.clock` is an additive optional object on the `health` envelope (and on the health block the audit and combined envelopes embed) with three required members: `source` (`environment`, `head_commit`, or `wall_clock`), `epoch_secs` (the reference instant in unix seconds), and `reproducible` (false only for `wall_clock`). Churn recency weighting, hotspot ranking, and ownership `stale_days` are all measured against one instant, and whether that instant is reproducible decides whether two runs over the same commit can be compared. The human report already said so in a stderr warning, which `--quiet` removes and a machine consumer never sees; the JSON consumer who most needs the answer was the one who could not get it. `head_commit` and `environment` (set via `FALLOW_CLOCK_EPOCH`) both resolve identically on every run over one commit; `wall_clock` is the fallback when no commit timestamp is readable, which is what imported churn (`--churn-file`) on a non-git tree hits. Pass `epoch_secs` back as `FALLOW_CLOCK_EPOCH` to reproduce a run's churn-derived numbers. The object is present whenever `hotspot_summary` is, and `hotspot_summary` is emitted whenever the run measured churn, which a default `fallow health --format json` over a git repository does; `--hotspots` adds the per-file `hotspots[]` listing beside it rather than gating the summary. So a health run over a git repository gains the `clock` key, and only a run with no readable git history (where `hotspot_summary` is absent entirely) stays byte-identical. No `schema_version` moves: nothing existing is renamed, removed, or retyped, no value joins an enum-valued required field, and no required field is added to a documented finding, which are the four conditions that bump. Treat the `source` value set as closed; it is the three resolution paths the clock has.
- **Baseline staleness reaches machine consumers**: `baseline_staleness` is an additive optional object carried at the `dead-code` / `check` envelope root (including the `dead-code-grouped` envelope), at the `dupes` envelope root (grouped and ungrouped, CLI and programmatic lineages), and inside `summary` on `health` (grouped and ungrouped), where it replaces the object of the same name that shipped in 3.12.0 and keeps every member it had. It is emitted whenever a `--baseline` was loaded and absent otherwise, so a run without a baseline is byte-identical. `fix` and `security` accept no baseline staleness and carry no object. Members: `baseline_entries`, `matched_entries`, `stale_entries`, `current_findings`, `change_scoped`, `stale`, `warning` (`none` / `zero-overlap` / `partial`), `gate_trips`, and `moved_entries`, which only `health` can ever set above zero: `dead-code` and `dupes` match entries by fingerprint and never classify one as moved, so they report `0`. `moved_entries` stays a required member on every carrier, so health's published shape is unchanged and no field is retyped. Until now the staleness verdict lived on stderr only, which `--quiet` removes, so the documented CI paths (the GitHub Action, the GitLab template, and every MCP tool) could not see it at all. Read `change_scoped` before dividing `matched_entries` by `baseline_entries`: a run narrowed to part of the project compares a whole-project baseline against a slice and can report `matched_entries: 0` on a perfectly healthy baseline, which is why both `stale` and `gate_trips` are false there. A narrowed run also carries `scope_reasons`, an additive optional ARRAY of kebab-case channel names present and non-empty exactly when `change_scoped` is true, absent otherwise, so a whole-project run stays byte-identical. Both members come from one predicate per command, so the boolean and the array cannot disagree. The names emitted today are `diff`, `changed-since`, `changed-files`, `workspace`, `changed-workspaces`, `scope`, `file`, `issue-type-filter` and `production`, serialized in that order so two identical runs produce identical bytes. Treat the name set as OPEN, the way `gate_outcomes` keys are open. Which names a command can emit differs per command and a consumer must not assume otherwise: `dead-code` reads the flags and can name every channel, while `dupes` and `health` see an already resolved changed-file set and report `changed-files`, and `health` reports `workspace` for both `--workspace` and `--changed-workspaces` because at that point it cannot tell them apart. `dupes` emits only `changed-files` and `production`, because its comparison runs before the report-narrowing filters and a `--workspace` or positional-path run still compares the whole project. Read the array to tell narrowing you can remove from narrowing you chose; `production`, `workspace` and `changed-workspaces` are the caller's own decision about what to analyze, and the GitHub Action and the GitLab template now use exactly that rule instead of guessing from their own inputs, which also makes scoping passed through `args` or `FALLOW_ARGS` visible to them. The Action publishes it as the new `baseline-scope-reasons` output, empty when the run was not narrowed or the binary predates the member. A run that loaded a non-empty baseline and was narrowed ONLY by channels a repeat can drop (`diff`, `changed-since`, `changed-files`, `scope`, `file`, `issue-type-filter`) also gains a `next_steps[]` entry with `id: "recheck-baseline"`, whose command re-reads that baseline over the whole project. One exception: the entry is withheld while `FALLOW_DIFF_FILE` is exported and the run was diff-scoped, because the command drops a flag and not the environment, so it would narrow again and offer itself again. Like every other entry it is runnable as-is and never mutating, so it re-reads and reports rather than re-saving. A run narrowed by `production`, `workspace` or `changed-workspaces` carries no such entry: the entry's command carries `--baseline` and nothing else, and those three channels resolve from the project config and the environment as well as from a flag, so the suggested command would come back just as narrow and re-emit the entry without end. That is the same removable-or-not split the two CI integrations apply, so a consumer reading `scope_reasons` and a consumer following `next_steps` reach the same conclusion. It is emitted on a zero-finding run too, which is exactly the run where a rotted baseline is otherwise silent, and it participates in the existing three-entry cap. `stale` and `gate_trips` answer different questions and legitimately disagree: `stale` mirrors the unasked-for stderr advisory, which stays silent below a quarter of the baseline and on a run that produced no findings at all, while `gate_trips` mirrors the opt-in `--fail-on-stale-baseline` rule, which fires on any unmatched entry AND on a baseline the reading command could not read as its own (`unrecognised_format: true`), because such a file protects nothing at all. Those two halves differ in one way: the unmatched-entry half stands down on a narrowed run, because a slice of the project cannot judge a whole-project baseline, and the recognition half does not, because which command wrote a file does not depend on how much of the project was analyzed. `gate_outcomes["stale-baseline"].status` still equals `gate_trips` on every single-analysis command, which is the identity that object exists for; `fallow audit` is the documented exception below, where the entry stands down for every baseline by design. A rotted baseline on a cleaned project therefore reports `stale: false` with `gate_trips: true`; that is the contract, not a defect, and `current_findings` is published so the divergence is explainable from the envelope alone. `gate_trips` describes the baseline, not the run's exit code: `health --report-only` is an explicit request never to fail, so such a run exits 0 and says so on stderr while still reporting `gate_trips: true`. The object is emitted with or without the flag. SUPERSEDED in part by the `gate_outcomes` bullet below: the flag also moves `gate_outcomes["stale-baseline"].enforced`, because whether a verdict is armed is part of the verdict. `baseline_staleness` itself, `gate_trips` included, is still identical with and without the flag. Dead-code's older `baseline` object (`entries`, `matched`) is untouched and keeps its meaning; new consumers should read `baseline_staleness`, whose `baseline_entries` and `matched_entries` always agree with it. `fallow audit` now publishes one object per baseline it loaded: at the root of its `dead_code` section (where the standalone `dead-code` envelope carries it, so a consumer's path is `<section>.baseline_staleness` on both), at the root of its `duplication` section, and inside its `complexity` section's own `summary`. Each is additive and optional, absent when that baseline was not loaded, and no `AUDIT_SCHEMA_VERSION` moves; the `duplication` object is not in the generated schema's `DupesReportPayload`, because a typed member there would collide with the `baseline_staleness` that `DupesOutput` flattens into the standalone envelope root, and the section validates it as an additional property. All three always report `change_scoped: true`, and `gate_trips: false` unless that section's baseline was one no command could read as its own, because every audit analyzes only the changed slice and the gate is inert on it by design, so nobody should build a gate on them. Their `scope_reasons` differ by sub-pass and are measured, not assumed: `dead_code` reports `changed-since`, because audit resolves its base ref and passes it to that sub-pass as a ref, while `duplication` and `complexity` report `changed-files`, because they receive an already resolved set. An audit that loaded any baseline also carries `gate_outcomes["stale-baseline"]` as `{status: "skipped", enforced: false}`: one entry for up to three baselines, for the same reason the CLI prints its inertness note once. That entry is what puts the fact in the `Gate outcomes:` line the sticky comment and the MR note already render, and in the MCP's gate sentences. Both integrations now emit one log line per audit baseline naming the unscoped command that can judge it (`duplication` points at `fallow dupes` and `complexity` at `fallow health`, because the section label is not the command), and both reject `--fail-on-stale-baseline` on an audit run wherever it was still reachable, which is through the `args` input and `FALLOW_ARGS`; the pair was already rejected through their baseline inputs. `fallow report --from` re-renders a saved envelope and carries the object only if the saved envelope already had it. No `schema_version` moves on any envelope: nothing existing is renamed, removed, or retyped, no value joins an enum-valued required field, and no required field is added to a documented finding, which are the four conditions that bump. The generated TypeScript contract renames the `HealthBaselineStaleness` interface to `BaselineStaleness` now that three commands share it, and ships `export type HealthBaselineStaleness = BaselineStaleness` as a deprecated alias so the old import keeps compiling. A loaded baseline written in another command's format now says so, on CLI stderr, in the Action's step log and job summary, in the GitLab job log, and in the MCP tools' `warnings` array, and carries `unrecognised_format: true` on the wire. That member is an additive optional BOOLEAN on `baseline_staleness`, present only when true, so an envelope from a run that loaded its own baseline is byte-identical to a pre-change one; read it as absent-means-false. Every verdict such a run derives was already green and honest (nothing to judge, so no entry could go unmatched), which meant a repository that pointed `--baseline` at a baseline another command saved gated on it forever without a word. All three commands set the member, on one rule: a saved baseline states which command wrote it in its top-level `kind`, and a file that names another command is not this command's baseline whatever it contains. A file that names none is a baseline saved before that member existed, and there the keys the file carries decide, measured against the keys the reading format writes. `dead-code` used to reject a foreign file with exit 2 instead, because five of its fields carry no serde default, so the same mistake crashed one command and went green on the other two; it now classifies the file before deserializing it, and invalid JSON or a dead-code baseline missing part of itself still exits 2. The dead-code `analysis_identity` rejection never caught this, because that field is itself defaulted and a syntactic run finds no incompatible fields. The decision is never `baseline_entries == 0`: a baseline saved from a project with nothing to record carries zero entries, is legitimately empty, and earns no note anywhere, which is what the documented save-on-a-green-main workflow produces. A consumer that wants the same distinction must read `unrecognised_format` for the same reason. The exit code moves only through `--fail-on-stale-baseline`, which such a file now trips: see the `gate_trips` rule above. A run that armed no gate keeps its exit code. The stderr note prints regardless of `--quiet`, names the command that saved the file when the file says so, because the fact appears in no human report and `--ci` implies `--quiet`. The Action publishes the member as the new `baseline-unrecognised` output, empty when no baseline was loaded, when the baseline belongs to the command that read it, or when the binary predates the member; neither integration gates the line on its own baseline input any more, so a baseline passed through `args` or `FALLOW_ARGS` earns it too, without a path to name. Treat the `warning` value set as closed: it is the three advisory outcomes the engine has.
- **Every gate a run armed reaches machine consumers**: `gate_outcomes` is an additive optional OBJECT keyed by gate name, carried at the envelope root of `dead-code` / `check` (grouped included), `dupes` (grouped and ungrouped), `health` (grouped and ungrouped), `audit`, `security`, and the bare combined run, where it sits at the root rather than per section, so the `.check.regression` versus `.regression` split stops mattering for the verdict. Combined mode accepts none of the health gate flags (`--min-score`, `--min-severity`), so the health sub-analysis arms nothing there but its default rule; the combined object carries the default rule of each section that ran (`error-severity-findings` for dead code, `health-findings` for health) and what combined mode can arm, which is `regression`, `stale-baseline`, `type-aware-require`, `parse-error` and `duplication-threshold`. `enforced` on that object is not the standalone commands' answer either: the combined machine renderers collapse every gate to exit 0 except the stale-baseline, regression, type-aware-require and parse-error gates, so the rest publish their verdict with `enforced: false` rather than claiming an exit the run cannot produce. On that object, for the default exit rules (`error-severity-findings`, `health-findings`), `status` gives the verdict of the human run of the same flags. An advisory entry can report `fail` without a failure of the human run: an example is a `stale-baseline` entry that `--fail-on-stale-baseline` did not arm. The CLI always emits the object on every envelope that has a default exit rule, also when no flag armed a gate, and it always contains that default rule: `error-severity-findings` on dead-code, check and combined, `health-findings` on health, `security-advisory` on security (`enforced` only when `--fail-on-issues` or an `error` rule severity lets it fail the run) and `audit-verdict` on audit. A JSON reader therefore sees a failing run without the exit code. `dupes` has no default exit rule: a `dupes` run that armed no gate exits 0 and carries no object, so there an absent object means the run passed. The object is never emitted empty. Before this change the object was emitted only when a flag or config ARMED a gate, so an older binary can still exit 1 carrying no object; see the behavior change below. A gate is armed by a flag or by config; the default rule is in the object whether or not anything armed it, so the object always explains the exit code it sits beside. Each entry has two required members, `status` (`pass` / `warn` / `fail` / `skipped`) and `enforced` (whether a `fail` from this gate makes the run exit non-zero), plus optional `observed` and `threshold` numbers where the gate compared one, and an optional `threshold_label` string where `threshold` does not carry its own unit. `observed` and `threshold` are always in the SAME unit, so the two can be compared directly: `regression` publishes the issue-count delta against the allowance that tolerance permits, not the tolerance's own number, and sets `threshold_label` to the tolerance as the user spelled it (`"50%"` or `"5"`) so a percentage survives onto the grouped envelope, which carries no `regression` object to read `tolerance_kind` from. `health-min-severity` sets `threshold_label` to the severity floor, so its count is recoverable from the entry alone. **A gate fails the build when `status` is `fail` AND `enforced` is true.** Neither member decides it alone: `enforced` is true on every armed gate including the ones that passed, so gating on it by itself fails every run that armed anything, and `status: fail` alone fails runs the CLI deliberately let pass. Read `status` on its own to decide what to say, remembering that `warn` and `skipped` are neither a pass nor a failure. The two legitimately disagree: `health --report-only` returns before any health gate is consulted, so every health entry on such a run reports `enforced: false` whatever its status (`type-aware-require` is not a health gate and stays enforced), and a stale-baseline verdict published without `--fail-on-stale-baseline` reports the same pair. Every entry is a projection of the rule that already decides the exit code, so `gate_outcomes["regression"].status` and `regression.exceeded`, `gate_outcomes["stale-baseline"].status` and `baseline_staleness.gate_trips`, and `gate_outcomes["security"].status` and `gate.verdict` cannot drift; tests pin each identity. The names emitted today are `error-severity-findings`, `regression`, `stale-baseline`, `duplication-threshold`, `health-min-score`, `health-min-severity`, `health-findings`, `health-coverage-gaps`, `health-runtime-coverage`, `security`, `security-advisory`, `audit-verdict`, `type-aware-require` and `parse-error`. Treat the key set as OPEN, the way `workspace_diagnostics[].kind` is open: a name this build does not recognise means "some gate", not an error, and a gate added later is an additive optional key that bumps nothing. `error-severity-findings` is the CLI's own severity rule and not a count: `--fail-on-issues` promotes warn-tier rules into it, so a project with a rule set to `warn` reports findings and still exits 0. `security-advisory` reports `skipped` on any run that configured `--gate`, because a configured gate returns before the advisory and a passing gate therefore suppresses it. `health` lists every gate the run evaluated, and `health-findings` is always among them unless `--min-severity` replaces it. The typed programmatic API runs no CLI-layer gate and leaves the field absent rather than emitting an empty object, which would assert that gates were evaluated and none tripped. `--ci` on its own selects SARIF and carries nothing; `--ci --format json` keeps the explicit format and does carry the object, and `--sarif-file` is the supported way to get both an envelope and a SARIF upload. No `schema_version` moves on any envelope: nothing existing is renamed, removed, or retyped, no value joins an enum-valued required field, and no required field is added to a previously documented finding. **One consequence supersedes a sentence above**: `--fail-on-stale-baseline` now moves exactly one wire member, `gate_outcomes["stale-baseline"].enforced`, because whether a verdict is armed is part of the verdict. The `baseline_staleness` object itself, `gate_trips` included, stays flag-independent, and nothing else in the report moves. The generated TypeScript declares it as `gate_outcomes?: (GateOutcomes | null)`, which admits a `null` the wire never produces: that is how the generator renders every optional object (`baseline_staleness` reads the same way), so guard on presence rather than writing a `null` branch you will never take. `GateOutcomes` is generated as `{ [k: string]: GateOutcome }` with no key union, which is the open key set above; `GateName` is published in the JSON Schema as the catalogue of names this build emits, and it deliberately does not constrain the map. It is referenced by nothing, so the generated TypeScript does not emit it: a TypeScript consumer gets `{ [k: string]: GateOutcome }` and finds the catalogue in the `gate_outcomes` field description instead, which lists every name. That is the cost of keeping the set open, and it is deliberate. This object is not the `gates` array on the pull-request decision surface (`fallow-pr-decision/v1`), and the two will not converge: that one carries `label`, `observed` and `threshold` as display text for the GitHub check run, this one is a machine verdict with no prose member at all. The display array is now DERIVED from this object for every entry, which is the bullet below; it remains a separate contract with its own members, and a consumer that needs the verdict reads this object rather than parsing display text.
- **Opt-in parse-error gate**: `--fail-on-parse-error` (on `dead-code`, `health`, `audit` and the bare run) and the top-level `failOnParseError` config key arm a new `gate_outcomes["parse-error"]` entry. The entry is `fail` when at least one file has a `source-parse-degraded` entry in `workspace_diagnostics[]`; other kinds that set `degrades_analysis` do not count. It carries `observed` (the number of such files) and a new optional `files` array on `GateOutcome`, one `{ path, error_count, panicked }` item per file, sorted by path, with the same facts as the diagnostic. `files` is absent on every other gate. The gate is never armed by default, so a run that does not opt in keeps its exit code and its envelope byte-identical, and the `source-parse-degraded` diagnostic does not change. An armed gate is enforced in every output format, also on the bare run, because the bare run applies it after the machine renderers; `health --report-only` publishes it with `enforced: false`. `audit` judges every file its sub-passes parsed, not only the changed files, so a file that was already unparseable fails every armed PR audit, also under `--gate new-only`; a per-path exemption for such files is a planned follow-up. The audit `verdict` does not include this gate: an armed audit can report `verdict: "pass"` and still exit 1, and `gate_outcomes` states the exit reason. The stderr gate lines print also under `--quiet` and `--ci`, because SARIF and CodeClimate have no place for the verdict. `dupes`, `fix` and a bare run that analyzes neither dead code nor health (`--only dupes`) reject the flag with exit 2. The GitHub Action and the GitLab template fail the job on an enforced `parse-error` failure: no input owns the gate, because the entry exists only when the flag or the config key armed it. The health score and `file_scores` do not change. Older binaries reject `failOnParseError` as an unknown config key, so a team that commits the key pins `minimumVersion` to the release that added it. No `schema_version` moves: the gate name joins an open key set and `files` is an additive optional member.
- **Deprecated exports in use** (2026-09-24): the dead-code envelope (grouped and ungrouped, CLI and programmatic) gains the `deprecated_exports_in_use` array and the required `summary.deprecated_exports_in_use` count. The CLI always emits both. The schema keeps the array optional, so a saved envelope from an older version still validates and loads through `fallow report --from`. The array holds a brand-new finding type, so no `schema_version` moves under the additive rule above. A validator that pins the exact key set of `summary` must add the new key. `unused_exports[]` and `unused_types[]` entries gain two additive optional fields, `deprecated` (always `true` when present) and `deprecated_reason` (a plain-text string of up to 200 characters). Both are absent when the export has no `@deprecated` tag, and `deprecated_reason` is also absent for a bare tag, so a run over code without the tag is byte-identical apart from the new array and count. The new `actions[].type` value `migrate-deprecated-export` appears only on the new finding type. The rule defaults to `off`.
- **Dead-code findings carry their gate severity** (2026-09-23): each dead-code finding may carry an additive optional `effective_severity` field, `error` or `warn`. The value is the rule severity after `overrides[].rules` resolve for the path of the finding (empty catalog groups and dependency overrides resolve for the file that declares them), and `--fail-on-issues` raises `warn` to `error`. The `fallow dead-code` findings gate fails when a finding is `error`; other gates (regression, stale baseline) decide on their own inputs. The `fallow audit` `new-only` gate fails only on introduced findings, so an inherited `error` finding does not fail the audit, and the combined command exits 0 for machine formats. The field is on the `dead-code` / `check` envelope, the `dead-code-grouped` envelope and the dead-code section of `audit` and the combined envelope. It is not in `required`, so `schema_version` does not change. It is not part of finding identity, baseline keys or fingerprints, so a change of rule severity does not make a finding new. SARIF, CodeClimate and `github-annotations` read the field in the direct run and in `fallow report --from`. When the field is absent (JSON from an older version), they use the earlier rule-based level. Policy violations keep their own `severity`. `prop_drilling_chains`, `thin_wrappers` and `duplicate_prop_shapes` never gate a run and do not carry the field.
- **Complexity findings carry their gate severity** (2026-09-23): each complexity finding (`findings[]` on the `health` envelope and on the health block of the `audit` and combined envelopes) may carry an additive optional `effective_severity` field, `error` or `warn`, next to the band in `severity`. The rules `complexity-cyclomatic`, `complexity-cognitive` and `complexity-crap` (default `error`) set it, with `overrides[].rules` resolved for the path of the finding. The most severe rule of the kinds in `exceeded` wins, and a finding whose contributing kinds are all `off` is not reported. The `fallow health` findings gate, the `health-findings` and `health-min-severity` entries of `gate_outcomes` and the audit verdict count only `error` findings; a `warn` finding gives audit verdict `warn`. The field is not in `required`, so `schema_version` does not change, and it is not part of health baseline keys, audit keys or fingerprints. SARIF, CodeClimate and `github-annotations` take the level of a complexity finding from the field in the direct run and in `fallow report --from`: this is a level change for default configs, because a blocking `moderate` finding now shows as `::error`, SARIF `error` and CodeClimate `major`. When the field is absent (JSON from an older version) or has an unknown value, the renderers keep the earlier band-based level. An older binary warns about the three rule names as unknown keys and ignores them.
- **A stale baseline reaches the review surfaces**: the sticky pull-request comment, the GitLab merge-request note and the two review bodies carry a baseline advisory clause in the status blockquote they already used for the type-aware message and the gate inventory. It is conditional: present when the run's `baseline_staleness` reports `warning` as `zero-overlap` or `partial`, or reports `gate_trips: true`, and absent otherwise, so a run with no baseline and a run whose baseline matched everything are byte-identical to a pre-change run. A multi-section envelope carries up to three baselines and gets one sentence per loaded baseline, each naming its section. The fact clauses match the GitHub job summary word for word; the remedy clause names a whole-project run rather than a channel, because one renderer serves both providers and cannot know whether the reader re-saves through the `save-baseline` input, `FALLOW_SAVE_BASELINE` or `--save-baseline`. The clause order inside the blockquote is type-aware message, then baseline advisory, then gate inventory, joined by a space on one line. `fallow report --from` produces the same bytes as the direct `--format` run, which the parity suite pins. One shape differs. A bare combined run renders its own multi-gate comment presentation. That presentation carries no baseline advisory clause. On a combined envelope the clause reaches a comment only through `fallow report --from`. The single-analysis commands carry it on both paths. The combined-comment bullet below states the full difference.

- **The pull-request decision surface lists every armed gate**: `fallow-pr-decision/v1`'s `gates` array gains one additive row per `gate_outcomes` entry, appended after the command row, which keeps its place and its `scope: "new code"`. A gate row carries `id` as the gate name exactly as the envelope spells it, a display `label` (an unrecognised name from a newer build degrades to its kebab spelling read as words rather than being dropped), `observed` as the numbers the gate compared or the status word when it compared none, `threshold` when the gate has one, and `scope: "this run"`, because a gate verdict is not scoped to the change. `status` maps `fail` plus `enforced` to `failure`, `fail` without `enforced` to `neutral`, `warn` to `neutral`, `skipped` to `skipped`, `pass` to `success`, and any status this build does not recognise to `neutral`, never to `success`. The surface's own `conclusion` is UNCHANGED: it stays derived from the command or per-area rows, so a tripped gate does not turn an advisory check into a merge blocker, and the integrations still own the failing exit. One consequence for `fallow ci post-check-run --split-gates`, which publishes one commit status per row: a repository using that flag gains a new `Fallow / <gate>` context per entry, and every run now has an entry for the default exit rule of its command. Nothing is renamed and neither integration passes `--split-gates`, so the default paths are unaffected.

- **`workspace_diagnostics[]` says which entries describe a degraded run**: each entry may carry an additive optional `degrades_analysis: true`, omitted when false. It is projected from the same classification that decides whether the CLI prints a stderr line for that kind, so a CI log built from this field and a local non-quiet run say the same thing. Read it instead of hardcoding a kind allowlist: a degrading kind added in a later release then reaches an unchanged consumer. The two unconfigured-check kinds (`boundaries-not-configured`, `rule-packs-not-configured`) answer false on purpose, because they fire in the product's default state on every project that never opted in and would otherwise warn forever; so does `excluded-by-default-ignore`, which reports designed behavior on generated output. A new open-set kind `no-source-files-analyzed` covers the case those exclusions used to hide: the run finished with no source file to analyze at all, so every finding count it reports is zero because nothing was measured rather than because the project is clean. It carries `excluded_file_count`, the built-in-ignore contribution, which is `0` when no pattern took part, because the condition also fires with no exclusion at all (a docs-only repository, a workspace member with no TypeScript, a path filter that matched nothing). It is recorded by source discovery rather than by the CLI's human note, so every envelope built from a diagnostics snapshot carries it, the MCP tools and the programmatic routes included; the human report's existing sentence is unchanged and the condition still produces exactly one line. Both are additive optional and no `schema_version` moves; the new kind is covered by the open-set exception this section already documents for `workspace_diagnostics[].kind`.

- **Degraded health inputs reach `workspace_diagnostics[]`**: seven new open-set kinds record a health input that did not load, each with the `message` and the next-step hint every kind carries, and each recorded whether or not `--quiet` was passed. `file-scores-unavailable` (scoring failed, so the score list is empty and the scored-file count is `0` because nothing was measured), `hotspots-skipped` (the hotspot, churn and ownership sections report nothing rather than zero; `cause` names which input stopped them, as `not-a-repository`, `invalid-since` or `churn-file-unreadable`, and the set is open like every other token set here. `no-commits` joined the set later for a current branch with no commit yet (issue #2803), with no `schema_version` move. Only the first shipped before the member existed, and its `message` is byte-identical to what it always was; `invalid-since` and `churn-file-unreadable` previously reached a `tracing` line and no consumer at all. The error text that distinguishes a malformed `--since` from an unparsable one stays on that line, because the two need one remedy. `cause` is a required member of that entry in the schema, the way `ownership-unavailable`'s is, so a consumer validating an envelope from an older build against a newer schema copy must follow this section's rule and validate against the schema shipped with the version it runs), `shallow-clone` (churn covers the fetched history only; `ownership_requested` says whether ownership was asked for and therefore skewed too), `unpinned-clock` (no commit timestamp, so churn recency and ownership staleness drift between runs over one commit), `ownership-unavailable` (`cause` is `invalid-bot-pattern` or `codeowners-parse-failed`), and `trend-snapshot-unreadable` (a saved snapshot the trend could not use) all answer `degrades_analysis: true`, because each is a result the run could not measure as asked. `coverage-auto-detected` answers false on purpose: nothing degraded, it is provenance, and `path` names the coverage file that fed the CRAP scores so a score computed against a file nobody chose can be reproduced. Every stderr line these replace is unchanged in wording; the auto-detected coverage note is now quiet-gated like every other health note instead of gated on `CI` being set, which printed it only where a consumer discards stderr and hid it from the human who could act on it. No consumer change is required, because the shipped integrations and the MCP tools select on `degrades_analysis` rather than on a kind allowlist. The kinds move no `schema_version` under the open-set exception for `workspace_diagnostics[].kind` documented above.

- **A config a framework plugin could not use reaches `workspace_diagnostics[]`**: two new open-set kinds record what a plugin read and could not act on, each carrying `plugin` (the plugin as it labels itself), `key` (the config key) and `reason` (a kebab-case token), with the config file in `path` like every other path-shaped diagnostic field, project-root-relative with forward slashes. No prose member: the `message` every kind carries is composed from those three. `plugin-config-unreadable` answers `degrades_analysis: true` and warns on stderr, because a declaration the user wrote did not reach the analysis: its `reason` set is `not-object-literal`, `array-form`, `spread` and `unreadable-entries`, and the Module Federation reader records it for an `exposes` or `remotes` declaration it could not read in full, naming the plugin that owns the config file (`webpack`, `rspack`, `rsbuild`, `vite`, `nextjs`) when the options were read from that plugin's own config and `module-federation` for a standalone `module-federation.config.*`. The `array-form` token reaches `remotes` only: the array form of `exposes` is read. `plugin-effect-not-modeled` answers false and warns on no channel, because nothing the run could measure was lost: its `reason` set is `key-effect-not-modeled` and `config-property-unreadable`, and the Nuxt auto-import gate records it for a `components:` or `imports:` surface that kept its convention entry patterns under `autoImports`, once per config file and surface, only where a pattern was actually retained. Both token sets are OPEN, the same tolerate-unknown-values contract `workspace_diagnostics[].kind` documents, and a token a consumer does not recognise means "some reason" rather than an error. One config file can hold two entries under one kind and one path (`exposes` and `remotes` both unreadable), so a consumer keyed on kind plus path must widen to the payload; the stderr dedupe and both registry folds already do. The kinds are recorded by the plugin stage, which runs in the dead-code prelude, so they reach the envelopes whose run includes a dead-code analyze pass (`dead-code` / `check` grouped and ungrouped, `health`, `flags`, `audit` under `dead_code`, the bare combined root) and not a standalone `fallow dupes`, the rule the analysis-stage kinds already follow. Plugin config parsing is not cached, so a warm cache carries them exactly like a cold run, and each run replaces the previous run's set so a fixed config drops out. `fallow list --workspaces` runs plugins on its own path and carries neither kind. No consumer change is required: the shipped integrations and the MCP tools select on `degrades_analysis` rather than on a kind allowlist, so the GitHub Action and the GitLab template report `plugin-config-unreadable` in their existing aggregated degraded-inputs warning and set their degraded output. That job-summary note now reads "Some findings or scores were computed over less than the whole project, or from an input that did not load", matching the analyze step's own wording, because not every degrading kind is about files. Exit codes are unchanged and no `schema_version` moves, under the open-set exception for `workspace_diagnostics[].kind` documented above.

- **Two more reasons for `plugin-config-unreadable`** (2026-09-23): the `reason` set of `plugin-config-unreadable` adds `unrecognized-call` and `import-target-unreadable`. The Module Federation reader records `unrecognized-call` when the options pass through a call that is not a known config wrapper (`createModuleFederationConfig` and `defineConfig` are known). The reader reads the object literal that the call receives as a lower bound, and records the reason against each Federation key that literal declares. It records `import-target-unreadable` when it follows a relative import or `require` and cannot read the target, against each Federation key that the readable part does not declare. Each token renders its own sentence, and neither remedy asks for an object literal. The set was already open, so no `schema_version` moves and no consumer change is required: the GitHub Action, the GitLab template and the MCP tools select on `degrades_analysis`, and a saved envelope that carries a new token loads in `fallow report --from`. A consumer that matches on the old four tokens sees a token it does not recognise, which means "some reason".

- **`--group-by` says so on every format that cannot carry it**: grouping is carried by `json`, `human`, `sarif` and `codeclimate`, and the remaining formats render one flat document. That fallback is unchanged and still exits `0`: it is a less useful report rather than a wrong one, and failing a run that passes `--group-by` globally across several formats would be a breaking change out of proportion to the defect. What changes is that it is no longer silent. `compact`, `markdown` and `badge` already printed a one-line stderr note; the four CI comment and review formats plus `github-annotations` and `github-summary` printed nothing at all, and they now print the same note, which names the format the way `--format` spells it. The four comment and review bodies additionally carry one clause stating the requested mode and pointing at `--format json`, joined to the existing note rather than replacing it, and `fallow report --from` renders the identical clause from a saved grouped envelope's `grouped_by`. No envelope changes shape and no `schema_version` moves: the fallback is decided at render time, and the one format with an envelope supports grouping, so there is nothing to record on the wire.
- **`fallow report --from` renders a saved run's gate verdicts and never its exit code**: the re-render reads `gate_outcomes` off the saved envelope and states what each gate concluded. Coverage is per format: `github-summary`, `github-annotations`, `pr-comment-github`, `pr-comment-gitlab` and the two review targets carry the line; SARIF and CodeClimate do not (see below). The line is informational on every surface. It never escalates to `::error::`, because the render cannot know which gates the consumer armed through its own inputs: `audit-verdict` is always enforced by the CLI while `command: audit` with `fail-on-issues: false` is a passing reporting job, so an error-level line there would paint a red annotation on a green run. The integration owns the failing exit. On the annotations target the line is emitted FIRST, because that stream is capped by the consumer and a verdict appended after the findings is the first thing a noisy run drops. It does not change the check-run `conclusion`, which stays the deliberate non-blocker it has been, and it does not reproduce the producing run's exit code: every `fallow report` render exits 0 and the caller owns the status, which is what lets a pipeline analyze once and render many without the render steps failing. SARIF carries no gate block (there is no `invocations` section to put one in) and CodeClimate is a bare issue array by specification, so neither states a verdict. Nothing is lost in transit: the re-render holds the saved envelope untyped and preserves every root key, so a gate field added later is present on every render path, grouped envelopes included, without a change here.
- **What a run was asked to do, and whether it did it, reaches machine consumers**: `request_outcomes` is an additive optional OBJECT keyed by request name, carried at the envelope root of `dead-code` / `check` (grouped included), `dupes` (grouped and ungrouped), `health` (grouped and ungrouped), `security`, `flags`, `suppressions`, and the bare combined envelope. On the combined envelope the ROOT is the only carrier, the same rule `workspace_diagnostics` follows there, so a run that skips a section still reports what it was asked for. `audit` carries none: it exits 2 rather than widen when its base ref will not resolve, and it states its own scope through `base_ref` and `base_description`. The object is emitted whenever the run RECEIVED at least one narrowing request and is absent otherwise, and an empty object is never emitted, so a run that asked for nothing is byte-identical to one produced before the object existed and no `schema_version` moves. Each entry carries `status` (`applied` or `not-applied`), `affects` (`scope` or `artifact`), `requested` (what was asked, as the user spelled it and never normalised, so it must not be joined to the project root the way every other path-shaped field is), and, exactly when `status` is not `applied`, `reason` (a kebab-case token) and `message` (one sentence naming the next step, byte-identical to the BODY of the stderr line for the same case: stderr prefixes `Warning: ` or `fallow: warning [diff-file]: `, and the envelope carries the sentence without the prefix). An entry may also carry `scope_size`, an additive optional non-negative integer present when the run applied the request AND measured what it left in scope, absent otherwise, including on every unapplied entry. The unit belongs to the NAME, not to the object: `diff-filter` counts added lines, and today it is the only name that measures anything, so read the unit off the key the entry sits under and never across names. Read an absent member as "not measured", never as zero. The count is what the run INDEXED rather than the diff's true total: `diff-filter` indexes at most one million added lines and reports that cap for a larger diff, so read any non-zero value as a lower bound. `0` is the case the member exists for: the request applied over an EMPTY scope, so every source-anchored finding filtered out and the clean report that follows covered nothing. That is why the empty scope stays `status: "applied"` rather than becoming a new status: all four shipped consumers gate on `status != "applied"` before reading `affects`, so a new status would have made them tell a reader that a run with nothing to analyze had reported more of the project than was asked for. All four consumers state that case once, beside the line they already print for an unapplied request: the GitHub Action and the GitLab template warn from the envelope they captured, the MCP tools add a sentence to `warnings[]`, and the comment, review, `github-summary` and `github-annotations` bodies carry a clause on their request-outcome line. An envelope from a binary that publishes no such member makes none of them say anything. Honoured requests are published too, so an absent object means "nothing was asked for" and never "nothing failed", and a reviewer can read "this report IS scoped to the change" positively. **Read `affects` before saying what an unapplied entry means.** On `scope` it means the report that follows is complete, valid, and WIDER than what was requested; on `artifact` it means a file the run was asked to write beside the report was not written, and the report's own scope is untouched. Neither ever changes the exit code. Select on `affects`, never on a name list: a request name added in a later release carries its own class, and a consumer that assumes the whole object narrows the report will tell its reader that an unwritten SARIF file widened the analysis. The class value set is OPEN like the others, and a class a consumer does not recognise must not be read as `scope`. The key set is OPEN and so is the `status` value set, the same tolerate-unknown-values contract `workspace_diagnostics[].kind` documents: a name or a status a consumer does not recognise means "some request" and "some outcome", not an error, and an unrecognised status must never be read as applied. The names this build emits are `changed-since` (reasons `git-missing`, `not-a-repository`, `git-failed`, and `invalid-ref`, which only the programmatic API can reach because the flag's own parser fails a malformed ref with exit 2), `diff-filter` (reasons `oversize`, `unreadable`, `not-utf8`, `foreign-namespace`, `ambiguous-base`) and `sarif-file` (reasons `directory-create-failed`, `write-failed`, `serialize-failed`). `sarif-file` is the one `affects: "artifact"` name today. `dead-code` / `check`, `dupes`, `health` and the bare combined run warn and continue on a failed write and publish either outcome. `security` owns a separate writer that still exits 2 with an error document on failure, so no envelope carries that entry, and it now publishes the honoured case: a `security` run that wrote the document reports `sarif-file` as `applied` with the path it wrote, which is what lets a consumer tell a run that produced the artefact from one that was never asked for it. `requested` is the `--sarif-file` path, an unapplied entry means nothing was written there so a consumer uploading that path to code scanning has nothing to upload, and the primary report on stdout and the exit code are unaffected, which is why nothing else about the run said so. A diff that parsed but names no analyzable file reports `applied`: the filter was applied over an empty scope, which is a different fact from a filter that stood down. Until now these facts lived on stderr only, which `--quiet` removes on the env-var diff channel entirely, so the documented CI paths (the GitHub Action, the GitLab template, and every MCP tool) could not see them at all. The Action publishes the unapplied `scope` names as its `requests-unapplied` output and warns once; the GitLab template writes the same list into `FALLOW_REQUESTS_UNAPPLIED` in `fallow-gates.env` and prints the same line. Both list narrowing requests only, so a workflow gating on either to detect an unscoped run cannot trip on a failed artifact write. `flags` and `suppressions` carry the `changed-since` entry and nothing else: the diff source is resolved once per process before the command runs, and neither applies a diff filter, so an entry for one would claim a narrowing that did not happen. Neither envelope moves its `schema_version` (`8` for `flags`, `1` for `suppressions`), because the object is absent on a run that passed no narrowing flag.
- **`fallow report --from` renders a saved run's request outcomes**: the re-render reads `request_outcomes` off the saved envelope and states what the run was asked to do, on the same formats that carry the gate line and with the same reasoning. The clause joins the existing note rather than replacing it, and the live `--format` render and the re-render produce byte-identical bodies for one envelope. It is informational and never escalates: on the annotations target it is a `::notice::`, because the Action already owns the warning for this fact and a second escalation would double-count against GitHub's ten-annotations-per-level budget. One exception to "reads it off the saved envelope": the comment and review targets state a `diff-filter` that stood down in the RE-RENDER's own process, overlaid on the saved object. Those two documents are filtered by the diff the rendering process was given, which in both shipped integrations is a diff their comment and review steps download themselves, so a stand-down there is a fact about the body being written and nothing else records it under `--quiet`. Only a stand-down is overlaid: a filter that applied claims nothing of its own, so a saved entry recording that the FINDINGS were computed at full scope survives, and a healthy render is byte-identical to what it produced before.
- **Dead-code findings can carry `reachability_caveats[]`**: nine arrays
  (`unused_files[]`, `unused_exports[]`, `unused_types[]`,
  `unused_enum_members[]`, `unused_class_members[]`, `unused_store_members[]`,
  `unused_dependencies[]`, `unused_dev_dependencies[]`,
  `unused_optional_dependencies[]`) may carry an additive optional array of
  kebab-case tokens. It appears on the `dead-code` and `dead-code-grouped`
  envelopes, under `check` on the bare combined envelope, and under `dead_code`
  on the audit brief. It records that the verdict rests on an import graph the
  run already knows is incomplete, because some source file's imports were never
  read. Five `workspace_diagnostics[]` kinds raise it: `source-parse-degraded`,
  `source-read-failure`, `skipped-large-file`, `skipped-minified-file`, and
  `skipped-source-dotdir`. The size skip is the one that fires at default
  settings, at 5 MB.

  Two tokens exist today: `incomplete-file-analysis`, meaning this finding's own
  file was not fully analyzed, and `incomplete-import-graph`, meaning some module
  feeding the verdict was not. Treat the set as open, the way
  `workspace_diagnostics[].kind` is open: a token this build does not recognise
  means "some caveat", not an error.

  Which findings carry which. `unused_files[]`, `unused_exports[]` and
  `unused_types[]` rest on reachability, so a degraded parse raises
  `incomplete-import-graph` on them only when the degraded module is itself
  reachable; a file the run never read raises it on all of them, because such a
  file has no graph node and its reachability cannot be observed. The member and
  dependency arrays rest on no reachability filter, so any incompletely analyzed
  file raises the token on every one of them. A dependency finding never carries
  `incomplete-file-analysis`, because the file it names is a `package.json`. No
  other issue array carries the field.

  What you must do: a caveated finding reports `auto_fixable: false` on its
  mutating action (`delete-file`, `remove-export`, `remove-enum-member`,
  `remove-class-member`, `remove-dependency`), with the reason in that action's
  `note`. Plan against `auto_fixable`, not against the presence of the action.
  Every mutation surface agrees with that flag: `fallow fix` withholds the write,
  the MCP `fix_preview` and `fix_apply` tools inherit that because they run it,
  and the LSP offers no `Remove unused export` and no `Delete this unused file`
  quick fix while a caveat stands.

  What stays the same: the finding is still reported, unfiltered, at the same
  severity, with the same number of actions in the same order (`actions[0].type`
  is unchanged) and its suppress alternative intact. Exit codes do not move. The
  array is omitted when empty, so a run that read every file it discovered is
  byte-identical and no `schema_version` moves. Store members carry the caveat as
  disclosure only: no surface offers a mutation for one, so there is nothing to
  withhold, but every surface that reports one still names it.
- **`fallow fix` withholds a mutation whose finding carries a caveat**: the
  write is declined, not attempted, and the decision is keyed on the presence of
  a caveat rather than on which diagnostic produced it.

  What you must do: gate on `skip_reason: "low_confidence_incomplete_analysis"`,
  which appears on the entry alongside its own `reachability_caveats` array. Do
  not parse the reason prose. Entry shapes differ by kind. A withheld dependency
  is a `remove_dependency` entry and a withheld member a `remove_enum_member` or
  `remove_class_member` entry, each with `applied: false` and `skipped: true` and
  each naming what it would have removed. Withheld export removals are reported
  as one `type: "skipped"` entry per file, naming no export and no line. The
  counters follow that split: `skipped_low_confidence_dependencies` and
  `skipped_low_confidence_members` count withheld entries, while
  `skipped_low_confidence_exports` counts files, so three withheld exports across
  two files report `2`. All three counters are always present and `0` on a clean
  run.

  What stays the same: the exit code, and the finding itself, still reported by
  `fallow dead-code` for manual confirmation. A class member's removal is opened
  only by the type-aware sidecar, so a project with no semantic pass sees no
  `remove_class_member` entries at all.
- **Every surface that recommends acting on a caveated finding names it**: the
  finding ships everywhere, unfiltered, with its `actions` array intact; only the
  mutation is withheld. All surfaces resolve the same labels through one shared
  helper.

  Where the text appears:

  - `--format json`, and the grouped, combined and audit envelopes: the
    `reachability_caveats[]` array itself.
  - human report: a dimmed ` (caveat: <labels>)` on the finding line, or, above
    the directory-rollup threshold, one section-level note counting the collapsed
    findings that carry one. `fallow check --summary` prints the same note once
    under the totals.
  - `--format sarif`: appended to the result message.
  - `--format codeclimate` and its `gitlab-codequality` alias: appended to
    `description`, which is the string GitLab renders inline on the merge-request
    diff. `pr-comment-github`, `pr-comment-gitlab`, `review-github` and
    `review-gitlab` all render from that description and inherit it.
  - `--format github-annotations`: appended to the end of the message, as a
    `Caveat: <labels>.` sentence and one line of explanation, after the
    remediation guidance.
  - `--format github-summary` and `--format markdown`: an italic
    `*(caveat: <labels>)*` inside the existing cell or finding line, so no table
    changes shape.
  - `--format compact`: a trailing `,caveat=<tokens>` field carrying the wire
    tokens rather than the prose labels, `+`-joined for more than one, appended
    after the colon-separated fields as `,fingerprint=` and `,group=` already are.
  - LSP diagnostics: the same parenthetical at the end of the message.
  - `fallow fix --format json`: `reachability_caveats` plus `skip_reason`.

  All three member arrays render it, `unused_store_members[]` included. Store
  members reached the JSON wire first and were rendered bare everywhere else, so
  a degraded run reported a caveated enum member next to a bare store member and
  the bare one read as better evidenced. A store-member finding on a degraded run
  now carries the same text as the enum and class members beside it on the human
  report and its `--summary` note, SARIF, CodeClimate (and the four PR-comment and
  review formats that render from its `description`), markdown, compact,
  `github-summary`, `github-annotations`, and LSP diagnostics. Class-member
  `github-annotations` gained it in the same pass. No envelope changes shape and
  no `schema_version` moves; a run that read every file it discovered is
  byte-identical.

  What you must do: `review-github` and `review-gitlab` withhold the
  ```` ```suggestion ```` block on a caveated finding wherever they would
  otherwise have rendered one, and print `No one-click fix offered` in its place,
  pointing at `workspace_diagnostics[]`. Automation that applies review
  suggestions therefore has nothing to apply on such a finding. Both the block
  and its replacement need the finding's source line, which these renderers read
  under `FALLOW_ROOT` (default: the working directory), so a run that passes
  `--root` without setting `FALLOW_ROOT` renders neither.

  What stays the same: `--format badge` carries no per-finding content (and
  `fallow check` refuses the format outright), and the `health`, `dupes` and
  `security` envelopes contain no reachability verdicts. SARIF
  `partialFingerprints` and CodeClimate `fingerprint` are computed from rule id
  and location, never from the message, so a finding that gains or loses a caveat
  keeps its identity and no resolved review thread reopens. Every string above is
  absent on a run that read every file it discovered.
- **Workspace-listing diagnostic paths**: `workspace_diagnostics[].path` on the `list-workspaces` envelope (`fallow workspaces --format json`, `fallow list --workspaces --format json`, `fallow list --format json`, and the MCP `project_info` tool) is now project-root-relative with forward slashes, matching the `workspaces[].path` field beside it. It previously emitted the absolute filesystem path, the one place in any fallow JSON envelope that did. The field's type is unchanged, so no `schema_version` moves; a consumer that treated this one path as absolute must join it onto the project root. A path outside the project root stays absolute.
- **Document-root structure**: every object-shaped `--format json` envelope covered by the typed root schema (`FallowOutput`) carries a top-level `kind` discriminator. Consumers should branch on `kind` instead of probing for unique field presence. The authoritative set of typed root kinds lives in `docs/output-schema.json`; the factual list below is checked against that schema manifest:
  <!-- fallow-output-kind-list:start -->
  `audit`, `explain`, `inspect_target`, `trace`, `trace-error`, `review-envelope`, `review-reconcile`, `coverage-setup`, `coverage-analyze`, `list-boundaries`, `list-workspaces`, `health`, `dupes`, `dead-code-grouped`, `impact`, `impact-cross-repo`, `security`, `security-survivors`, `security-blind-spots`, `dead-code`, `combined`, `feature-flags`, `audit-brief`, `decision-surface`, `review-walkthrough-guide`, `review-walkthrough-validation`, `suppression-inventory`, `doctor`, `type-aware-status`, `similar-code`, `similar-code-inspect`, `similar-code-review`
  <!-- fallow-output-kind-list:end -->
  A saved baseline file also carries a top-level `kind`, and it is NOT one of these root kinds: a baseline is not a report, its token set is the three commands that save one, and its contract is the separate bullet under **CLI interface** below.
  Tagged root envelopes are now the only supported object-shaped JSON contract. The CLI `check` command is a legacy alias for `dead-code`; new JSON discriminators use the canonical `dead-code` name. `CodeClimateOutput` stays as a sibling root branch because the Code Climate / GitLab Code Quality spec requires a bare JSON array at the root; discriminate it by checking whether the document root is an array. Helper/spec JSON roots outside `FallowOutput`, such as `fix`, `fallow config`, non-boundary `fallow list` modes, SARIF, CodeClimate, telemetry, the `audit-cache remove` and `audit-cache prune` maintenance envelopes, and baseline/config files written by fallow, are not part of this envelope contract. The two audit-cache envelopes still carry their own `kind` and `schema_version` fields and follow the same additive-only evolution policy.
- **Security survivor schema**: `security-survivors` uses schema version `2`; `summary.unverdicted` is required and reports candidates without matching verifier verdicts.
- **Duplication stats under `--top`**: the `dupes` envelope moves to schema version `10` and the programmatic duplication envelope to `4`. `stats.clone_groups` and `stats.clone_instances` now describe the corpus the run measured, exactly like `stats.files_with_clones` and `stats.duplication_percentage` beside them. Before, `--top N` truncated `clone_groups[]` and then rewrote those two counters from the truncated array while the other two stayed corpus-wide, so one object reported two different scopes: on a sample project `9`/`19`/`12`/`4.7268` became `2`/`4`/`12`/`4.7268`. Two required integers replace what the rewrite used to encode: `clone_groups_shown` is the length of `clone_groups[]` and `clone_groups_omitted` is what a presentation cap withheld, so `clone_groups_shown + clone_groups_omitted == stats.clone_groups` always holds and is `0` on an untruncated run. Scope filters (`--changed-since`, `--workspace`, a duplication baseline) still recompute `stats` against the narrowed corpus and therefore omit nothing. A consumer that read `stats.clone_groups` as "how many groups are in this array" reads `clone_groups_shown` instead. `fallow dead-code --format json` is deliberately unchanged: `--top` stays human-output-only there, because that envelope drives exit codes and CI baselines and a consumer can slice a JSON array itself. The family axis carried the identical defect and is fixed in the same version, so no `schema_version` moves again: `--top` rebuilds `clone_families[]` from the groups that survive the cap, and on a sample project the array fell from `163` families to `3` with nothing on the envelope recording the drop, no corpus-wide family counter to compare it against, and a human notice that named the group axis only. A new required integer `stats.clone_families` counts the families the scoped corpus holds after filtering, and two new required integers `clone_families_shown` and `clone_families_omitted` split it exactly as the group pair does, so `clone_families_shown + clone_families_omitted == stats.clone_families` always holds and is `0` on an untruncated run. `stats.clone_families` is present on every duplication `stats` object, including each `--group-by` bucket, where it counts the families overlapping that bucket. Human output moves with the wire: the default report's `Duplicates (N clone groups)` header and its withheld-groups footer now name the measured corpus instead of the capped listing (under `--top 3` that project printed `3` as the project total and dropped the footer entirely), the footer additionally names the families a cap withheld, and the `--summary` block's withheld-notice covers both axes and states both corpus totals. The footer reads `... N of M clone groups withheld by a display limit` and `... N of M clone families withheld by --top`: the two axes are narrowed by different mechanisms, so neither line claims the other's limit, and the wording holds at `--top 0`, where nothing was listed. Both human clean states test the measured corpus rather than the length of `clone_groups[]`, so `--top 0` no longer prints `No code duplication found` (or, under `--summary`, `No duplication found`) over a run whose `stats.clone_groups` is non-zero. Markdown output moves with them: `## Fallow: N clone groups found (P% duplication)` counted the listing while the rate beside it described the corpus, so the heading now counts the corpus and an italic line below it names how many groups and families a display limit withheld.
- **`dupes --top` and `--group-by` are refused together**: `fallow dupes --top N --group-by <mode>` used to be accepted, silently drop `--top`, and exit 0 reporting every clone group with `clone_groups_omitted` at `0`, which reads as a run that withheld nothing rather than a flag that was ignored. It now exits `2`, the repository's invalid-input code, with a message naming why: a `--group-by` bucket reports `stats` computed over every clone group in that bucket, so a global top-N truncation would leave those per-bucket numbers describing groups the output no longer lists, the exact two-scopes-in-one-object defect the `clone_groups_shown` / `clone_groups_omitted` pair was introduced to remove. Either flag on its own is unchanged, no envelope changes shape, and no `schema_version` moves. A caller that passed both should drop one; to rank inside a bucket, run `--group-by` and sort `groups[].clone_groups[]` client-side. Combined mode and `fallow audit` never set `--top` on their duplication sub-run, so they are unaffected.

- **Feature-flag envelope carries `workspace_diagnostics[]`**: `fallow flags --format json` gained the same optional array the `dead-code`, `dupes`, `health`, and `security` envelopes carry, with project-root-relative paths and `#[serde(skip_serializing_if)]`, so a run that records no diagnostic is byte-identical and `schema_version` stays at `8` under the additive-optional-field rule above. It was the one analysis command whose envelope had no key for them: a measured run on a project with an oversized file, an unparseable file, and no installed dependency tree reported `skipped-large-file`, `source-parse-degraded`, and `node-modules-missing` on `fallow dead-code --format json` and nothing at all on `fallow flags --format json`, while stderr carried only the parse warning. Each of those is a reason a flag is absent from `feature_flags[]`. The analysis-stage kinds (`malformed-pnpm-workspace-yaml`, `bun-lockb-override-resolution-skipped`, `boundaries-not-configured`, `rule-packs-not-configured`) appear on this envelope too, because the flag scan correlates findings with dead exports and therefore runs the dead-code analyze pass that records them; the measured run above reported both unconfigured-detector kinds under `fallow flags` once the array existed. Consumers pinning the exact key set of this envelope should widen it by one. The remaining envelopes with no such key are `similar-code`, `impact`, `inspect_target`, `suppression-inventory`, and the `trace` family; they are unchanged here.

- **Duplication fragments are suppressible**: `clone_groups[].instances[].fragment` (and the same field inside `clone_families[].groups[]` and under `--group-by`) is omitted when the caller asks for a location-only payload: `fallow dupes --no-fragments`, or the MCP `find_dupes` tool, whose new `include_fragments` parameter defaults to false. The CLI keeps emitting the text by default, so the unchanged path is byte-identical. The field is now additive-optional rather than required. Each instance still carries `file`, `start_line`, `end_line`, `start_col`, and `end_col`, which address the same code. Fingerprints, `suggested_name`, refactoring suggestions, and `actions[]` are all computed before serialization, so suppressing the text changes no other value. Because the field moves from required to optional on the shared `CloneInstance` shape, every envelope that embeds it bumps with it: the `audit` envelope moves to `11`, the bare combined envelope to `12`, and the duplication envelope itself, whose bump to CLI `10` and programmatic `4` is recorded in the duplication-stats entry above rather than twice. Neither of those paths can suppress the text today, so their wire stays byte-identical; the bump records that the contract no longer guarantees the key, and a consumer pinning the exact instance key set should widen it.
- **Error envelope carries `code` and `help`**: the `--format json` error document (`{"error": true, ...}`) may now carry an additive optional `code` (a stable machine-readable identifier such as `unknown_issue_type`) and `help` (a remediation hint) beside `message` and `exit_code`. Both are omitted when the failure has neither, so the unchanged path is byte-identical and no envelope version moves. Commands whose failure already had a structured code (`fallow explain` on an unknown issue type, the `fallow similar-code` subcommands) stop flattening the three fields into one message string. Human output appends the hint on its own `hint:` line.
- **Unknown selectors suggest the nearest token**: `fallow explain <token>`, the Code Mode `fallow.run(tool, ...)` host call, the MCP `analyze` tool's `issue_types` entries, an unknown `fallow://` resource URI, `fallow rule-pack init --template <name>`, and every Node-API string option with a fixed literal set (`mode`, `sort`, `ownershipEmails`, `effort`, `typeAware.require`) answer an unrecognized name with the registered token one or two edits away instead of a fixed example list. The suggestion uses the same bounded, deterministic matcher as config rule-name typo detection and stays silent when nothing is close, so a novel token keeps the previous message. Each message keeps its existing prefix and its full list of accepted values; the suggestion is inserted, not substituted. A second, structural matcher now answers the miss edit distance cannot reach: a caller who spells out a name fallow abbreviates. `unused-dependencies` is seven edits from `unused-deps` and used to answer with a bare dump of the whole vocabulary; it now names `unused-deps`. It aligns the candidate's words against the caller's, so a longer name that merely shares a prefix (`unused-dependency-overrides`) does not win, and a token with no shared word stays unanswered. The MCP `analyze` refusal also gained the typed fields every other fallow refusal carries: `code: "unknown_issue_type"` (the same code `fallow explain` uses), a `help`, and `context: "analyze.issue_types"`, so an agent branches on the code instead of matching prose. Exit codes are unchanged.
- **MCP `inspect_similar_code` snapshot is an open object**: the `snapshot` parameter's input schema no longer inlines the candidate-snapshot shape, which was the single largest input schema in `tools/list`. The shape is published once as the new `fallow://schema/similar-code-snapshot` resource, and the typed check moved into the handler: a snapshot that is not a candidate snapshot is refused with `FALLOW_MCP_INVALID_CANDIDATE_SNAPSHOT`, `exit_code: 2`, and a `help` pointing at that resource, rather than a raw parser message. The parameter keeps its `"type": "object"` constraint: without it the published schema accepted a string or a number and only the server said no, so a client-side validator could not refuse what the handler would. A caller passing the snapshot back unchanged, as documented, sees no difference.
- **MCP output-limit refusal carries its measurement**: a subprocess response over the byte cap used to return a contentless tool error. It stays a tool error, with `isError`, `error: true`, and `exit_code: 2` intact, and the body now also carries `ok: false`, `truncated: true`, `result_bytes` (what the stream actually produced), `result_preview` (its first bytes), `limit_bytes`, `stream`, and `context: "subprocess"`, beside the existing `code: FALLOW_MCP_SUBPROCESS_OUTPUT_LIMIT`. The measurement field names match the Code Mode result refusal so an agent parses one shape on both surfaces, and both surfaces answer `isError` the same way for the same class of refusal: an agent gating on `isError` cannot read a 256-byte preview as a completed analysis. The `help` no longer suggests raising `max_output_bytes`, which is impossible: the parameter only LOWERS the 16 MiB default. Subprocess-backed MCP tools accept that optional `max_output_bytes` per call; the 16 MiB default is unchanged.
- **MCP resources gained two entries**: `fallow://schema/similar-code-snapshot` (static) and the `fallow://tools/{name}` template, which carries the per-flag detail (payload shapes, unit vocabularies, suppression placements) moved out of a tool's `tools/list` description. `fallow://tools` stays the terse one-line-per-tool catalogue. The `check_health` wire description is the first to be split this way; its content is preserved, relocated, and reachable at `fallow://tools/check_health`.
- **MCP guide and trace refusals are distinguishable**: `fallow://tools/{name}` used to answer a misspelled name and a registered tool that simply has no guide with byte-identical bodies. A registered tool now returns `code: "no_tool_guide"` with `registered_tool: true`, and a name that is not a fallow MCP tool returns `code: "unknown_tool"` with `registered_tool: false`; both keep `nearest_matches`, `documented_tools`, and `index`. A caller that matched on `no_tool_guide` for any missing guide should widen to either code, or read `registered_tool`. Separately, the MCP `trace_error` tool stopped pre-validating an empty `trace` itself, so that refusal is the API's typed one: `code: "FALLOW_INVALID_TRACE_OPTIONS"`, a `help`, and `context: "trace_error"`, matching the oversized-trace refusal beside it. The message and `exit_code: 2` are unchanged.
- **MCP shared parameter descriptions are one sentence**: the six parameters nearly every tool carries (`root`, `config`, `allow_remote_extends`, `workspace`, `no_cache`, `threads`) describe themselves in one sentence, worded identically on every tool that takes them, including the `symbol_impact` schema mirror that had drifted to its own phrasing. Two facts the trim removed are back, because each one bounded behavior the shortened sentence left undiscoverable: `allow_remote_extends` states again that it defaults to false and never grants process-global trust, the only statement bounding a trust-boundary flag, and `workspace` names the repo-relative path again, because patterns still match against both the package name and that path and the unmatched-pattern refusal lists only names. Only the `description` prose changed: no parameter was added, removed, renamed, retyped, or given a different default, no `schema_version` moves, and no request that was accepted before is refused now.
- **MCP `max_output_bytes` describes the refusal it actually performs**: on the twenty subprocess-backed tools that take it, the parameter's schema description said a response over the cap came back as `truncated` plus a preview. The behavior is and was a refusal: `isError`, `error: true`, `exit_code: 2`, and no analysis, which is what the output-limit entry above already records. An agent that lowered the cap to bound its context planned on a bounded result and got a full run that returned nothing. The description now says the call is REFUSED and names `isError` and `exit_code: 2`. The Code Mode `code_execute` parameter of the same name is a different cap on host-call output and keeps its own wording. Only the `description` prose changed: no parameter was added, removed, renamed, retyped, or given a different default, no behavior changed, no `schema_version` moves, and every request accepted before is accepted now.
- **MCP tools that write a file no longer declare a read-only hint**: `analyze`, `check_changed`, `find_dupes` and `check_health` write a baseline, regression baseline or snapshot file when the caller passes `save_baseline`, `save_regression_baseline` or `save_snapshot`. Their `tools/list` annotations change from `readOnlyHint: true` to `readOnlyHint: false` with an explicit `destructiveHint: false`, because MCP defaults `destructiveHint` to true when `readOnlyHint` is false. The `read_only` member of the matching `fallow schema` `mcp_tools` rows and the `fallow://tools` resource rows changes from `true` to `false`. Every parameter stays. A host that approves read-only tools with no prompt now asks for approval on these four tools. `code_execute` stays read-only: a host call inside it that passes one of the three write parameters is refused before dispatch with `error_kind: "invalid_params"` and a message that names the standalone tool. The refusal spends no `max_host_calls` slot. An empty `save_baseline` or `save_regression_baseline` is not refused, because it writes nothing. Before, the call wrote the file. The `save_regression_baseline` description no longer says that an absent value writes to the fallow config, because the MCP tools never passed an absent or empty value to the CLI.
- **`github-summary` lists dead-code rows in one order for the live and the saved render**: each dead-code section of the job summary sorts its rows by path (the source file of a boundary violation, the first file of a cycle), then line, then the serialized item. A saved `--group-by` envelope listed its rows group by group under `fallow report --from`, while the live render listed them in flat path order. Both renders now show the flat order. An ungrouped run keeps its row order where the envelope already sorts by path and line. Rows with no path (duplicate exports, unlisted dependencies) sort by name. `github-summary` and `github-annotations` are now in the saved-render byte-parity suite; the suite masks only the elapsed-time token in the summary header, because two processes never share one value.
- **The combined pull-request comment carries the status note**: the comment body of bare `fallow --format pr-comment-github` and `pr-comment-gitlab` now carries the same status note as `fallow report --from` on the saved envelope: the type-aware clause, the baseline advisory, the gate inventory and the request outcomes, in that order, as one blockquote line under the callout. The Check Run summary in the decision sidecar carries the same note. A run with nothing to state renders the body as before.


#### Cyclomatic metric populations

Health vital signs include optional `cyclomatic_population` metadata alongside
`avg_cyclomatic`, `p90_cyclomatic`, and `critical_complexity_pct`. Its `functions`,
`modules`, and `templates` groups each carry a unit `count`, cyclomatic `sum`, and
nullable `max`. Sum the groups' sums and divide by their counts to reconstruct
the mean before rounding. The percentile and critical share use that same
population. Module-scope units contribute to aggregates without creating
function findings. Existing threshold, suppression, and baseline behavior is
unchanged. Older snapshots omit the metadata, indicating unknown population
rather than zero units. No metric formula or envelope version changes.

#### Report ordering and colliding duplication handles

Health complexity findings retain the requested descending metric priority.
Ties use ascending project-relative path, line, column, then function name, so
discovery order and checkout location do not change which tied finding `--top`
selects.

Ordinary `dup:<8hex>` handles and widened `dup:<16hex>` handles retain their
content identity. Groups sharing the full content hash use report-scoped
`dup:<16hex>-rN` handles. Their ordinal is assigned from canonical fragment,
location and metric ordering, independent of the checkout's common path prefix.
Changing the collision bucket can still change its report-scoped ordinals.

Legacy numeric collision handles (`dup:<16hex>-N`) remain valid input syntax,
but are not aliases for the corrected handles. Old collision suppressions and
baseline keys therefore resurface their findings rather than silently selecting
a different group. Regenerate the report, review the affected group, and refresh
its `ignoredClones` key or baseline; obtain a current handle before tracing it.
Unsuffixed handles are unaffected. Update all analyzer installations before
storing `-rN` suppression keys, because older versions reject that config syntax.
Use `minimumVersion` to pin a shared config's required released version.

The report field types and envelope versions are unchanged. This corrects
location-dependent ordering and introduces a distinguishable collision handle
to prevent unsafe reuse of the previous ordinal assignment.

#### Pinning the output JSON Schema

The committed `docs/output-schema.json` carries a stable top-level `$id`:

```
https://raw.githubusercontent.com/fallow-rs/fallow/main/docs/output-schema.json
```

To pin a specific revision, replace `main` with a release tag (for example `v2.75.0`) or a commit SHA in your own vendored copy of the URL. Pinning to a tag is stable across rebases; pinning to `main` tracks the latest committed schema.

ajv and other JSON Schema validators do NOT fetch `$id` over the network by default. The URL functions as a deduplication key when registering multiple schemas in one process (`ajv.addSchema` keys by `$id` when present) and as a base URI for `$ref` resolution. Vendoring the schema body into your own toolchain is supported; you may rewrite `$id` to your own scope if your pipeline registers multiple revisions in parallel.

Minimal ajv strict setup:

```ts
import Ajv from "ajv";
import schema from "./docs/output-schema.json"; // or your pinned copy

const ajv = new Ajv({ strict: true, allErrors: true });
const validate = ajv.compile(schema);

if (!validate(fallowOutput)) {
  console.error(validate.errors);
  process.exit(1);
}
```

For TypeScript types generated from the schema, see `npm/fallow/types/output-contract.d.ts` (mirrored to `editors/vscode/src/generated/output-contract.d.ts`). The npm package also exposes `fallow/capabilities.json`, a version-matched copy of `fallow schema` with CLI capability metadata, and `fallow/issue-registry.json`, a narrow issue registry export derived from the same source. Regenerate the full bundle with `npm run generate:contracts`.

The legacy TypeScript `SchemaVersion` alias remains equivalent to
`CheckSchemaVersion` for source compatibility. Version-gated consumers should
use the concrete envelope's `schema_version` field or its specific generated
alias, such as `HealthSchemaVersion` or `CombinedSchemaVersion`.

#### TypeScript bare-name backwards-compat aliases

The schema-derive ladder ([#384](https://github.com/fallow-rs/fallow/issues/384), [#408](https://github.com/fallow-rs/fallow/issues/408), [#409](https://github.com/fallow-rs/fallow/issues/409)) wrapped every bare finding type in a `*Finding` envelope (`UnusedExport` to `UnusedExportFinding`, `CloneGroup` to `CloneGroupFinding`, etc.). The wrappers flatten the bare finding's fields via Rust's `#[serde(flatten)]` and add `actions[]` (and, where the wrapper participates in `fallow audit` attribution, the optional `introduced` flag), so the JSON wire shape is byte-identical.

`json-schema-to-typescript` drops the orphan inner definitions when every field is subsumed by a flattening parent (even with `unreachableDefinitions: true`), so the bare names disappear from the generated `.d.ts` unless they are aliased back explicitly. The npm-published `fallow/types` subpath (`npm/fallow/types/output-contract.d.ts`) carries an alias for every wrapper so external consumers importing the bare names continue to compile. The full list lives at the end of the generated file under the `// Backwards-compat aliases` section, with per-alias JSDoc explaining the migration history.

**Stability commitment**: legacy output aliases remain supported throughout v3. Removing them requires an explicit deprecation period and a future major release. New code that consumes fallow's JSON output should import the `*Finding` wrapper names directly.

### CLI interface

- **Subcommands**: `dead-code` (legacy alias: `check`), `dupes`, `health`, `audit`, `security`, `explain`, `fix`, `watch`, `doctor`, `init`, `hooks`, `agent`, `setup-hooks`, `migrate`, `list`, `schema`, `config-schema`, `plugin-schema`, `config`, `coverage`, `license`, `ci`. `security` is opt-in (the `security-client-server-leak` rule defaults to `off`); its findings never appear under bare `fallow` or `audit`.
- **`coverage` subcommands**: `setup`, `analyze`, `upload-source-maps`, `upload-inventory`. `analyze` accepts `--runtime-coverage <path>` for local mode and `--cloud` / `--runtime-coverage-cloud` (or `FALLOW_RUNTIME_COVERAGE_SOURCE=cloud`) for explicit cloud-pull; `FALLOW_API_KEY` alone never selects cloud mode.
- **`license` subcommands**: `activate`, `status`, `refresh`, `deactivate`, `trial`. JWT verification is offline-only; `activate` and `refresh` are the only network-touching operations.
- **Default behavior**: bare `fallow` (no subcommand) runs dead-code + dupes + health combined
- **Exit codes**: 0 (success/no errors), 1 (issues with error severity found), 2 (validation or runtime error), 3 (a requested resource is unavailable because `config --path` found no config or a license is unavailable or invalid), 4 (the runtime coverage sidecar is unavailable, unverifiable, protocol-incompatible, or terminated unexpectedly), 5 (runtime coverage input could not be prepared or parsed), 6 (the runtime coverage sidecar reported an internal error), and 7 (network or cloud request failed). `coverage upload-inventory` and `coverage upload-static-findings` use 10 for invalid input or project state, 11 for an oversized payload, 12 for rejected authentication or authorization, and 13 for a server failure after retries. `coverage upload-source-maps` retains the general 1, 2, and 7 meanings. `fallow audit` defaults to `--gate new-only`, so inherited error-severity findings in changed files can be reported with exit 0; use `--gate all` to fail on every finding in changed files. `fallow security --gate new` and `fallow security --gate newly-reachable` add exit code **8**, dedicated to a security candidate matching the selected gate mode (changed-line candidate or newly entry-reachable candidate). A gate that cannot compute its required diff or base tree exits 2, not 8. These codes are stable so pipelines can pin them (for example GitLab `allow_failure: exit_codes: [8]`). The official GitHub Action exposes the same gate through `security-gate`, and the GitLab template exposes it through `FALLOW_SECURITY_GATE`.
- **Global flags**: `--format`, `--config`, `--workspace`, `--production`, `--no-production` (force production mode off, overriding a project config's `production: true`; conflicts with `--production`), `--baseline`, `--save-baseline`, `--baseline-mode` (health baseline matching: `count` per file and category, the default, or `identity` per function identity for strict regression gates; an identity comparison requires a baseline saved with `--baseline-mode identity`; a save that omits the flag refuses to overwrite a baseline carrying identity buckets, and an explicit `--baseline-mode count` downgrades it on purpose), `--no-cache`, `--threads`, `--changed-since` (alias: `--base`), `--churn-file` (import a `fallow-churn/v1` JSON change-history file for hotspots/ownership/targets on non-git VCS), `--performance`, `--explain`, `--ci`, `--fail-on-issues`, `--sarif-file`, `--output-file` (alias: `-o`; write the report to a file instead of stdout, for any `--format`), `--fail-on-regression`, `--fail-on-stale-baseline` (exit 1 when a loaded `--baseline` has entries matching nothing this run; stricter than the advisory staleness warning, applies in every `--format` including the bare combined run, inert without a baseline, on scope-narrowed runs including production mode, under `health --report-only`, and on `fallow audit`, which only ever analyzes changed files; an inert run says so on stderr rather than passing quietly, and the exit code and stderr line are the only things the flag changes: report envelopes are byte-identical with and without it), `--tolerance`, `--regression-baseline`, `--save-regression-baseline`, `--summary`, `--group-by` (owner, directory, package, section), `--include-entry-exports`, `--max-file-size` (skip source files larger than N megabytes at discovery, default 5, `0` disables; declaration files are always analyzed), `--dupes-mode`, `--dupes-near`, `--dupes-threshold`, `--dupes-min-tokens`, `--dupes-min-lines`, `--dupes-min-occurrences`, `--dupes-skip-local`, `--dupes-cross-language`, `--dupes-ignore-imports`, `--dupes-no-ignore-imports` (count module wiring in combined mode; opt out of the default exclusion)
- **Per-analysis production flags**: `--production-dead-code`, `--production-health`, `--production-dupes` (bare combined mode and `fallow audit`)
- **Bare command flags**: `--only`, `--skip` (select which analyses to run), `--coverage` (Istanbul coverage data for the embedded health analysis), `--coverage-root` (absolute coverage-data prefix for CI rebasing), `--score` (health score in combined mode), `--trend` (compare against snapshot), `--save-snapshot` (save vital signs for trend tracking)
- **Health flags**: `--score` (project health score 0-100 with letter grade), `--min-score` (CI quality gate), `--max-cyclomatic` / `--max-cognitive` / `--max-crap` (per-function complexity thresholds; CRAP combines complexity with coverage), `--targets` (refactoring recommendations), `--effort` (filter targets by effort level: low/medium/high), `--coverage-gaps` (static test coverage gaps), `--coverage` (Istanbul coverage data for accurate CRAP scores), `--coverage-root` (absolute coverage-data prefix for CI rebasing), `--save-snapshot` (saves vital signs snapshot for trend tracking), `--trend` (compare against most recent snapshot)
- **Audit flags**: `--gate <new-only|all>` (controls whether only introduced findings or all findings affect the verdict), `--max-crap` (forwarded to the health sub-analysis; mirrors `health.maxCrap` in config), `--coverage` (Istanbul coverage data for accurate CRAP scores; falls back to `FALLOW_COVERAGE`, then `health.coverage`), `--coverage-root` (absolute coverage-data prefix for CI rebasing; falls back to `FALLOW_COVERAGE_ROOT`, then `health.coverageRoot`), `--no-css` (disable audit styling analytics), `--css-deep` (force deep styling analytics on when config disables it), `--no-css-deep` (skip project-wide styling reachability while keeping local styling checks)
- **Security flags and subcommands**: `--gate <new|newly-reachable>` (security candidate regression gate, exit code 8 on a matching candidate), `--surface` (include attack-surface inventory), `--file <path>` (candidate scope, also accepted after `security blind-spots`), `--runtime-coverage <path>` (runtime ranking signal), `--min-invocations-hot <n>` (runtime hot-path threshold), `security survivors --candidates <file> --verdicts <file> --require-verdict-for-each-candidate` (render verifier-retained survivor candidates, with optional complete-verdict gate), `security blind-spots` (group unresolved callee blind spots)
- **Init flags**: `--toml`, `--hooks` (scaffold pre-commit git hook), `--branch` (fallback base branch/ref for the hook when no upstream is set)
- **Hooks command**: `hooks install|uninstall --target <git|agent>` manages Git pre-commit hooks and agent gates. `setup-hooks` is deprecated: it keeps working throughout v3 with a stderr warning and is removed in the next major; `fallow agent install` or `hooks install --target agent` replace it.
- **Doctor command**: `doctor` performs deterministic, local, read-only readiness checks without analysis, cache writes, telemetry, network access, dependency installation, third-party execution, or report-file writes. It checks `root`, `config`, `workspaces`, `plugins`, `type-aware`, `dependencies`, `cache`, and `graph-cache` in that stable order; the three new checks are appended, so the existing five keep their positions. Human and JSON output are supported on stdout; `--output-file` is rejected to keep diagnosis inputs immutable. The JSON envelope has `kind: "doctor"`, `schema_version: 2`, `root: "."`, aggregate `status`, summary counts, and typed `checks[]`; remediation commands carry `cwd: "."` and declare whether they are `mutating`. A safe project-relative explicit config is retained in remediation commands. For an external or unsafe-to-render config path, Doctor omits the command and instructs the caller to reuse the same private `--config` value. Pass and advisory warning outcomes exit 0. A failed required check still emits the complete envelope and exits 2. Analysis and gate flags are rejected instead of being silently ignored. `schema_version` moved to 2 for the `dependencies`, `cache`, and `graph-cache` checks. All three are advisory (`required: false`) and none can fail: `dependencies` warns when the project has no `node_modules` directory and is not a Deno project that runs without one, `cache` warns when a persisted extraction cache exists but would not be reused, and `graph-cache` warns when a persisted module graph exists but could not be loaded. Both cache checks name the reason and the on-disk size. The two caches are reported separately because a run reuses them independently: an extraction cache can be perfectly reusable while the whole graph is rebuilt every run, and the graph blob is the larger of the two on a real project. A project with no cache yet passes. A project without installed dependencies therefore reports the aggregate `warn` where it previously reported `pass`, still exiting 0. `dependencies` reuses the existing `project` category; `cache` and `graph-cache` introduce a new `checks[].category` value, `cache`, so a consumer pinning the exact category set should widen it by one.
- **Agent command**: `agent install|status|uninstall` is the one-pass onboarding for Claude Code, Codex, and Cursor (`AGENTS.md` task map plus a `CLAUDE.md` import, the fallow skill, MCP server registration, and the agent gate). It composes `init --agents` and `hooks install --target agent`, which stay supported as the single-piece commands. Every file or block it writes carries a versioned `fallow:agent-install` marker; `--dry-run`, `--force`, `--user`, `--approve`, and `--without <guide|skill|mcp|hooks>` are stable flags, and the JSON envelope (`kind` of `agent-install`, `agent-uninstall`, or `agent-status`, `schema_version` 2, `fallow_version`, `root`, `steps[]` with `status`, `scope`, `path`, `reason`, `next_actions[]` with a `mutating` flag) is versioned. The step `reason` values (`skill_name_taken`, `skill_not_embedded`, `mcp_entry_unavailable`, `mcp_entry_foreign`, `machine_local_launcher`, `approval_not_requested`, `settings_local_tracked`, `manual_command`, `unsupported_harness`, `user_scope_unsupported`, `user_edited`, `invalid_json`, `invalid_toml`, `not_an_object`, `managed_block_malformed`) are part of that contract. A `fallow` MCP entry is treated as fallow-managed only when its command matches a launcher fallow writes; other entries are refused (`mcp_entry_foreign`) unless `--force`, and `--force` on an unparsable config file saves the old bytes next to it as `<file>.fallow-bak` before rewriting. `schema_version` moved to 2 because `agent status` now reports two gate failure modes it previously rendered as `installed`. A `hooks` surface whose gate script was written by an older fallow reports `stale`, the comparison `skill` surfaces already made; and an installed gate whose run-time prerequisites are missing reports `stale` too, with the reason in `detail` and a matching entry in `next_actions[]`. The two new `next_actions[].id` values are `gate-requires-jq` (the script exits 0 after one stderr line when `jq` is absent, which a Claude Code PreToolUse hook never surfaces) and `gate-path-version` (the gate runs the `fallow` that PATH resolves, not the build that installed it). The three envelopes share one constant, so `agent-install` and `agent-uninstall` move to 2 with no other change. No field was added, removed, or retyped, and `state` gained no value: a consumer keyed on `installed` versus `stale` now sees `stale` on a machine where the gate would not have gated.
- **Environment variables**: `FALLOW_FORMAT`, `FALLOW_QUIET`, `FALLOW_BIN`, `FALLOW_TIMEOUT_SECS`, `FALLOW_EXTENDS_TIMEOUT_SECS`, `FALLOW_COVERAGE`, `FALLOW_COVERAGE_ROOT`, `FALLOW_CACHE_DIR`, `FALLOW_API_URL`, `FALLOW_API_KEY`, `FALLOW_CA_BUNDLE`, `FALLOW_PRODUCTION`, `FALLOW_PRODUCTION_DEAD_CODE`, `FALLOW_PRODUCTION_HEALTH`, `FALLOW_PRODUCTION_DUPES`, `FALLOW_REVIEW_GUIDANCE`, `FALLOW_REVIEW_ID`, `FALLOW_SUMMARY_SCOPE`, `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS`, `FALLOW_UPDATE_CHECK`, `FALLOW_MAX_FILE_SIZE` (per-file size limit in megabytes, mirrors `--max-file-size`; `0` disables), `FALLOW_SUGGESTIONS` (set to `off`/`0`/`false`/`no`/`disabled` to suppress the `next_steps[]` array in JSON output and the human `Next:` line; default on)
- **CI comment formats**: `pr-comment-github`, `pr-comment-gitlab`, `review-github`, and `review-gitlab` are stable machine-oriented markdown/envelope formats for bundled CI integrations. Wording, grouping, and markdown presentation can improve in minor releases, but marker comments, review fingerprints, and documented control variables such as `FALLOW_SUMMARY_SCOPE`, `FALLOW_REVIEW_GUIDANCE`, `FALLOW_REVIEW_ID`, `FALLOW_BOT_LOGIN`, and `FALLOW_MAX_COMMENTS` remain compatible. Scoped envelopes carry `meta.review_id`, and generated summary, inline, and resolution bodies repeat the exact scope marker so reconciliation cannot cross review jobs; unscoped jobs see only unscoped bodies.

  Duplication issues in CodeClimate output carry the inclusive `location.lines.end`
  plus `other_locations[]` ranges for the other instances in the clone group. Both
  fields are additive and omitted from non-duplication point findings. Inline review
  comments render those peer ranges with repository-root-relative paths, identify the
  clone with its stable `dup:` handle instead of its report-order ordinal, and anchor
  an added-mode comment to the first added line inside the matched range.

  Inline finding reconciliation follows explicit Fallow lifecycle state. An owned discussion without a Fallow resolution marker deduplicates a current finding even if a reviewer manually resolved the provider thread, and receives exactly one marker reply when the finding disappears. A marker closes that lifecycle permanently; a provider-reopened old thread is closed again without another reply, while a current recurrence receives a fresh discussion that can later be resolved once independently. SHA-qualified and legacy bare resolution markers remain compatible, but SHA inequality alone is never treated as force-push or recurrence evidence. Unattached legacy GitHub markers are associated best-effort with the nearest preceding open lifecycle in provider creation order because payloads without a root id cannot prove their original generation. When `FALLOW_BOT_LOGIN` is set, only that exact posting username authenticates root findings and resolution replies. When unset, GitHub uses provider-native bot metadata, while GitLab also matches notes to the authenticated token owner returned by its current-user API. The variable does not identify lifecycle generations.
- **Review envelope conclusions**: `fallow-review-envelope/v3` defines `meta.check_conclusion` as the quality-gate result independently of whether inline findings were selected. A passing gate can therefore carry review-visible findings, and an incomplete required analysis can fail without an inline finding. Versions v1 and v2 retain their historical finding-derived conclusion semantics.
- **Health config fields**: `health.coverage` and `health.coverageRoot` are stable fallbacks for standalone health, bare combined mode, `fallow audit` (both the head pass and the base attribution pass), and the MCP `audit` / `check_health` tools (their typed route and their CLI fallback alike) when the matching CLI flag or tool parameter and the env var are omitted. A relative `health.coverage` resolves against the analysis root on every surface, including the programmatic existence check. Structured errors keep their codes: a missing map is `FALLOW_INVALID_COVERAGE_PATH`, whose `context` always reads `health.coverage` whichever layer supplied the path, while a relative root is `FALLOW_INVALID_COVERAGE_ROOT`, whose `context` names the supplying layer. The Node-API bindings are the documented exception: they take `coverage` / `coverageRoot` as explicit options only and read neither the env vars nor the config fields, so embedders that want the shared precedence resolve it themselves before calling.
- **Generated hook-script env vars**: `FALLOW_GATE_MIN_VERSION` (consumed by
  the generated `fallow-gate.sh` in the target project's Claude hooks
  directory; written by `fallow hooks install --target agent` or
  `fallow setup-hooks`; controls the minimum fallow version the gate accepts;
  the default is hand-bumped each release, with the generated
  `crates/cli/src/setup_hooks/fallow-gate.sh` as the source of truth; empty
  string disables) and `FALLOW_GATE_DEBUG` (any
  non-empty value makes the same script log a stderr notice when it skips a
  command it does not classify as a `git commit` or `git push`)

- **Saved baseline files state which command wrote them**: every baseline `--save-baseline` writes carries a top-level `kind` string of `dead-code`, `dupes` or `health`, spelled exactly as the matching envelope root kinds. Treat the token set as CLOSED: it is one token per command that saves a baseline, and a command added later adds a token. The member is additive and the file format is otherwise unchanged, in both directions. A baseline saved by an earlier release carries no `kind` and loads on all three commands exactly as it did, with the keys it carries deciding which format it is. A baseline saved by this release loads on a binary that predates the member, because no baseline format rejects unknown fields; a `kind` value a newer fallow writes loads too, because the member is never read back through the typed structs. The three formats' own key lists deliberately do not include `kind`: it is the one key all three write, so listing it would make every format declare every other one.

  A load whose `kind` names another command warns on stderr, naming both commands and the path, carries `unrecognised_format: true`, suppresses nothing and trips `--fail-on-stale-baseline`. It does not fail a run that armed no gate. `fallow audit` checks every baseline it loads and reports every mismatch, not the first, and its own `gate_outcomes["stale-baseline"]` still stands down because no audit judges a baseline.

  A `--save-baseline` whose destination carries a different `kind` is REFUSED with exit 2, naming both commands and the path, because a save rewrites the whole file and would destroy it; the remedy is one path per command, and there is no override flag. A destination with no `kind`, an unreadable one and an absent one are all overwritten silently, a destination this command wrote is overwritten silently, and a re-save therefore adds `kind` to a file saved by an earlier release with no note.

- **Combined comments render through `fallow report --from`**: the bare combined run's `pr-comment-github` and `pr-comment-gitlab` bodies are NOT reproduced byte-for-byte by `fallow report --from` over that run's saved envelope, unlike every single-analysis command's. Combined mode renders a richer multi-gate presentation the generic saved renderer has no input for, so `report --from` produces the generic body for a combined envelope. The machine formats (`codeclimate`, `sarif`) are byte-identical on combined as everywhere else. A repository that needs the combined presentation renders it from the direct run. The parity suite pins both halves of this, so the difference cannot change silently.

### External plugin format

- **Plugin file structure**: as documented in `docs/plugin-authoring.md`
- **Detection types**: `dependency`, `fileExists`, `all`, `any`

### Type-aware protocol

- **Stable starting point**: wire protocol version 6 is the first stable
  contract between Fallow and the optional `fallow-type-aware` companion.
  Version 7 adds the closed `svelte-virtual-module-exports` semantic gap reason.
- **Exact-version pairing**: the native binary and companion package must have
  the same Fallow version. The backend version and supported operations come
  from `crates/api/type-aware-protocol.json`.
- **Evolution**: additive response fields may be introduced when older readers
  can ignore them. Removing or changing an operation, envelope, or required
  field requires a new wire protocol version and a documented compatibility
  path.
- **Pre-stable protocols**: development-only protocols before version 6 are
  rejected and are not part of the compatibility guarantee.

### Coverage inventory upload blob

`fallow coverage upload-inventory` posts a versioned JSON body. The `version`
field is informational: every field added after version 1 is optional and
omitted when empty, so the shape itself is the compatibility mechanism and a
reader never branches on the version to parse the body.

- **Version 1**: `gitSha` plus `functions[]`, each carrying `filePath`,
  `functionName`, `lineNumber`, and the protocol `identity` block. A version 1
  body stays valid and stays readable; nothing was removed or retyped.
- **Version 2**: adds optional per-function `cyclomatic` and `cognitive`
  (`u16`, McCabe and SonarSource respectively, omitted when the walk could not
  pair a function to a complexity result) and an optional `churnByPath` map
  keyed by the same `filePath` shape `functions[]` uses.
- **Version 3**: adds an optional `callerEdges` map keyed by the callee's
  `identity.stable_id`, each entry listing the importer `file` values and the
  `symbols` they import. Import-edge granularity, not file:line call sites.
  Emitted only for `upload-inventory --with-callers`.

Version 3 bodies also carry a `callerEdgeLimits` header alongside `callerEdges`:
`maxSitesPerFunction` and `maxSymbolsPerSite` name the size guard the producer
applied, and `truncatedFunctions` counts callees whose importer list was cut. A
reader needs all three to report a fan-in honestly, because a callee with
exactly `maxSitesPerFunction` importers is indistinguishable from a truncated
one without the count. The header is absent whenever `callerEdges` is, so a
version 1 or version 2 body keeps its exact wire shape.

Complexity, churn, importer edges, and the size-guard header are descriptive
context. None of them gates a verdict, an actionability decision, or a
confidence level.

## What may change in minor/patch versions

These are explicitly **not** covered by the stability guarantee:

- **New fields** may be added to config, JSON output, or plugin format (additive changes)
- **New issue types** may be added
- **New plugins** may be added to the built-in set
- **Detection accuracy**: false positive/negative rates may improve
- **Churn-derived numbers**: recency-weighted commit scores, hotspot ranking, `stale_days`, bus-factor and ownership signals, and the routing experts computed from them are functions of the analyzed history and of where the window boundary falls. They move when history moves. They are reproducible for one commit (see the behavior change below), not stable across commits or across releases that change the weighting.
- **Human-readable output**: terminal formatting, colors, wording
- **Performance characteristics**: timing, memory usage, parallelism
- **SARIF output details**: beyond what the SARIF spec requires
- **LSP protocol details**: diagnostics, code actions, Code Lens behavior
- **Rust crate APIs**: all workspace crates, including `fallow-api`, are
  integration surfaces for Fallow's own CLI, MCP, NAPI, and editor adapters,
  not supported external semver APIs. Their Rust types and functions may change
  in a minor release. Stable consumers should use the versioned JSON, CLI,
  npm, or protocol surfaces documented above. `fallow-api::runtime_json`
  remains an internal protocol bridge; new command families expose typed
  `run_*` output before adding JSON at protocol boundaries.

## Deprecation process

When a stable interface needs to change:

1. The old behavior is deprecated with a warning in the current major version
2. The new behavior is available alongside the old one
3. The old behavior is removed in the next major version

## Notable behavior changes within v3

These are documented for the rare CI script that depended on the old behavior. None require a config migration.

- **The GitHub Action rejects a control character in the `baseline` input.**
  A `baseline` value with an ASCII control character, for example a newline,
  now stops the analyze step with exit 2 and an `::error::` line, as the
  `changed-since` and `diff-file` inputs already did. The `baseline_path`
  output of the internal analyze step now uses the `name<<delimiter` form of
  `$GITHUB_OUTPUT`. The public action output `baseline-path` keeps the same
  value. The branded token step now also writes `FALLOW_TOKEN_BRANDED` and
  `FALLOW_TOKEN_FALLBACK_REASON` (empty for a branded token) to
  `$GITHUB_ENV`. These names are internal to the action and can change.

- **Every machine envelope states its default verdict in `gate_outcomes`.**
  The JSON envelopes of `dead-code`, `check`, `health`, `security`, `audit`
  and bare `fallow` (grouped and ungrouped) now always carry
  `gate_outcomes`, with the default exit rule of the command in it, also
  when no flag armed a gate. Before, the object was absent unless a flag or
  config armed a gate, so a JSON reader could not tell a failing run from a
  passing one without the exit code. No exit code changes: bare `fallow` in
  a machine format still exits 0 for findings, and its default rules report
  `enforced: false`. A script that reads the presence of `gate_outcomes` as
  "a gate was armed" must read the entries instead. The consumer rule is
  unchanged: a gate fails the build when `status` is `fail` AND `enforced` is
  true. The GitHub Action and the GitLab template treat `health-findings`
  like `error-severity-findings`: the count gate owns it, so it adds no log
  line. The action outputs `gates-failed` and `gates-passed`, the GitLab
  dotenv `FALLOW_GATES_FAILED` and `FALLOW_GATES_PASSED`, and the job-summary
  `Gates:` line now list the default rule on every run. A step that checks
  `gates-failed != ''` now also matches a run with findings and
  `fail-on-issues: false`. Such a step must read the gate names it wants, or
  the `enforced` member in the envelope. `fallow report --from` and the pull-request decision surface now show
  a line or a row for the default rule, and the MCP tools that wrap the CLI
  add a `warnings` sentence when it fails. `dupes` has no default exit rule and
  keeps the old presence rule. The typed programmatic API still leaves the
  object absent.
- **Bare `fallow` applies dupes and health baselines.** Bare `fallow` now
  accepts `--dupes-baseline` and `--health-baseline`, the names `audit`
  uses, and applies them as `dupes --baseline` and `health --baseline` do.
  The two flags configure bare `fallow` only. Before a subcommand they stop
  the run with exit 2, as `--coverage` does.
  The combined JSON carries `baseline_staleness` in its `dupes` section and
  in its `health` section `summary` when the baseline loaded.
  `--fail-on-stale-baseline` on a bare run now judges every loaded baseline,
  not only the dead-code one, and `gate_outcomes["stale-baseline"]` reports
  `fail` when any of them tripped.

- **A per-file `overrides` entry decides the exit code of three manifest
  rules.** An `overrides` entry for `package.json` or `pnpm-workspace.yaml`
  can set the severity of `unused-dependency-overrides`,
  `misconfigured-dependency-overrides` and `empty-catalog-groups`. The exit
  code of `fallow dead-code` and `check`, and the `--gate all` verdict of
  `fallow audit`, now use that severity. Before, they used the base `rules`.
  The `--gate new-only` verdict already used the override. Two results
  change. A run with an
  override to `warn` over a base `error` now exits 0. A run with an override
  to `error` over a base `warn` for a dependency-override rule now exits 1.
  The findings do not change. To keep the old exit code, set the severity in
  the base `rules` and remove the `overrides` entry for that rule.

- **`fallow dead-code --file` hides a duplicate export that only ignored
  files hold.** `ignoreFindings` hides a `duplicate-exports` finding only
  when every file that exports the name matches. After `--file`, the finding
  keeps only the files that you name. When every one of these files matches
  `ignoreFindings`, the finding is now hidden, as `--changed-since` and
  `--workspace` already did. Before, `--file` showed it. The JSON field and
  the finding shape do not change. A script that counted this finding after
  `--file` now counts one less.

- **`fallow audit` reports dependency findings only when their manifest
  changed.** A dependency-level finding (an unused, type-only, test-only or
  misplaced dependency, or an unused catalog entry) belongs to the
  `package.json` or the catalog file that declares it. Audit now keeps such a
  finding only when the changeset touches that file, for the root manifest
  and for each workspace package manifest. Before, audit reported every
  dependency finding of the project, as inherited or as introduced, because
  `--changed-since` keeps dependency findings whatever changed. The rule
  applies to the base snapshot too. One result changes: when a source edit
  makes a dependency unused and the manifest does not change, audit does not
  report the finding, and the `new-only` gate does not fail on it. Before, the
  finding had no base key, so audit marked it as introduced and the gate
  failed. `fallow dead-code --changed-since` does not change: it still
  reports dependency findings for the whole project. The MCP `audit` tool and
  `fallow_api::run_audit` now run the same audit as the CLI, so they give the
  same result, and they follow renamed files the same way. The typed
  `base_snapshot` now holds the keys of the base run scoped to the changed
  files and the pre-rename paths, with the rename remap and the dependency
  scope applied. No field is renamed, retyped, or added, and no
  `schema_version` moves. A CI script that counted dependency findings in the
  audit output sees fewer of them. Run `fallow dead-code` to see every
  dependency finding.

- **Agent-facing JSON now applies `rules` and per-path `overrides[].rules`.**
  The programmatic runtime behind the MCP `analyze` and `check_changed` tools,
  the audit sub-analyses, Code Mode's combined run, and the Node bindings
  resolved effective severities nowhere, so a finding on a path whose override
  turned its rule off was reported there while `fallow dead-code` and the editor
  suppressed it. All of them now run the same engine pass the CLI runs. A
  consumer that relied on the wider result set sees those findings disappear;
  turning the rule back on for the path restores them. The default severities
  apply there too: a project with no config no longer sees findings for rules
  that default to off, such as `private-type-leaks`, in the programmatic
  payload, which is what the CLI has always shown. The MCP `decision_surface`
  tool resolves its base snapshot with the head configuration, as `fallow
  decision-surface` does, so a rule flipped on in the change under review does
  not frame a decision for an edge that already existed at base. Separately,
  `fallow dead-code --type-aware` now applies
  the pass again after type-aware reconciliation, so a private-type leak that
  only the semantic pass discovers is subject to an override on its path just
  like a syntactic one. No field is renamed, retyped, or added, and no
  `schema_version` moves.

- **Security finding IDs include the anchor column.** The shared JSON,
  visualization and security SARIF identity hashes rule, normalized relative
  path, line and column. The old rule/path/line tuple merged distinct sinks on
  the same line, which also made their verifier verdicts ambiguous. Security
  SARIF names the corrected algorithm `fallowSecurity/v2` and does not emit the
  old `fallowSecurity/v1` key. Other fingerprint families are unchanged.
  Every security ID changes on upgrade. Regenerate candidates and their
  verdicts together, and refresh ID-based evaluation labels. Historical
  candidate/verdict pairs remain usable together; do not remap their verdicts
  to new candidates by path and line alone. Stored review history does not
  transfer automatically, and security alerts may close and reopen once.
  This identity correction changes no JSON field shape or envelope version.

- **Every SARIF result in one run now carries its own
  `partialFingerprints` value.** GitHub code scanning treats that value as
  alert identity, so two results sharing one were shown as a single alert and
  the second finding was never surfaced. The value was rule id plus URI plus a
  normalized source snippet, which is identical for two findings of the same
  rule on the same line: every re-export in a one-line barrel, every member of
  a one-line enum, and every dependency in a compact `package.json` collapsed
  into one alert. CodeClimate never had this, because it keys a dependency on
  the package name. Two things changed. The 1-based start column now takes part
  in the value, next to the snippet, exactly as it already did on the path with
  no snippet; the line still does not, so a finding that an edit above it moves
  keeps its identity and no triaged alert reopens. And a dependency result now
  reports the column of its own key inside the manifest line instead of a
  constant `1`, which is both what makes the fingerprints differ and a more
  accurate location for the annotation GitHub renders. A run-level pass then
  guarantees the property outright: where two results still compute the same
  value, such as a file that declares the same export twice with byte-identical
  text, the first keeps the value it computed and each repeat mixes in its
  occurrence index. Consumers that stored fingerprints from an earlier version
  see each affected alert close and reopen once; the format, the keys, and the
  16-hex shape are unchanged, and no `schema_version` moves. In a `package.json`
  written on one line a dependency's snippet is the whole file, so editing any
  dependency there still moves the others' fingerprints; a manifest with one
  dependency per line, which is what every package manager writes, is
  unaffected.

- **`fallow fix --format json` reports the manifest of a `remove_dependency`
  entry as a project-relative path.** The entry's `file` field carried the
  absolute path of `package.json` on the machine that ran the analysis, while
  the sibling `remove_export` entries in the same `fixes` array carried a
  project-relative `path`, so one array mixed two path spaces and an agent
  following fallow's project-root-relative output contract read a location that
  does not exist on its side. `file` is now root-relative with forward slashes
  on every platform, matching `remove_export`, the catalog fixers, and the
  `Would remove` / `Kept` stderr lines. Applied, dry-run, and withheld entries
  all changed together. A manifest outside the project root keeps its full
  path, since it has no relative form. No key was added or removed and no field
  changed type, so no `schema_version` moves; a consumer that fed `file`
  straight to the filesystem should join it to the project root first, as it
  already does for `remove_export`.

- **`fallow trace` gained a second question: `--path <FROM> <TO>`.** The
  symbol positional is now optional and mutually exclusive with `--path`, which
  reports the shortest chain of imports by which one module reaches another. It
  is an additive `kind: "trace"` payload carrying its own
  `schema_version: "1"`, independent of the other (unversioned) trace shapes,
  and it is reachable over MCP as `trace_import_path`. Two answers are not
  errors and both report `hops: 0`: an unreachable pair (`reachable: false`)
  and the same module on both sides (`reachable: true`), so a consumer must
  branch on `reachable`, never on the hop count. Type-only hops are reported
  with `type_only: true` rather than skipped. Equal-length routes resolve to
  the lexicographically smallest file-id sequence, so the JSON is byte-identical
  across runs. Only an endpoint that is not a module in the graph exits 2.

- **`fallow trace-error [FILE|-]` is a new command.** It reads a runtime stack
  trace from a file or from stdin (`-`, or no argument), recognises the
  V8 / Node `at name (file:line:col)` form and the SpiderMonkey /
  JavaScriptCore `name@file:line:col` form, and resolves each frame's
  identifier against the module graph. It is an additive
  `kind: "trace-error"` payload carrying its own `schema_version: "1"`,
  independent of the other trace shapes, and it adds one value to the typed
  root `kind` set; no existing envelope moves. Every frame read stays in
  `frames[]` in input order with an `origin` of `in_project`, `node_modules`
  or `out_of_corpus`, and a `resolution` of `resolved`, `ambiguous`,
  `not_found` or `not_attempted`. A frame matching several definitions is
  `ambiguous` and lists all of them rather than choosing one; a frame matching
  none is `not_found` rather than absent; a frame the graph was never asked
  about (a dependency, a runtime internal, generated bundle output, or a frame
  with no identifier to look up) is `not_attempted`. `counts` publishes the
  per-outcome totals, and both
  `resolved + ambiguous + not_found + not_attempted` and
  `in_project + node_modules + out_of_corpus` equal `counts.frames`, so a
  consumer can verify nothing was dropped. No source maps are read: a frame
  inside `dist/`, `build/`, `out/` or `.next/` that matches no analysed module
  reports that reason instead of being rebound through a map that may be
  stale. An empty trace, and a trace in which nothing is recognised as a frame,
  are ANSWERS: both exit 0 with `frames: []`, the latter reporting the lines it
  could not read in `counts.unparsed_lines`. Only unreadable input, input over
  the 1 MiB limit, or a failed analysis exits 2. An absolute frame path is
  resolved against the project root with symlinks followed on both sides, so a
  project reached through a symlink (macOS `/tmp`, a checkout linked into
  place) resolves the same frames as the canonical spelling; a path that does
  not exist is never rewritten. A `resolved` frame whose own line sits at a
  different declaration in the same file carries `line_mismatch: true` and says
  which declaration in `reason`, because the look-up matches on the identifier
  alone. Human output prints `counts.frames_omitted` and
  `counts.unparsed_lines` on the counts line whenever they are non-zero, so
  `--quiet` cannot make a capped trace look complete or an unrecognised input
  look empty. The same resolution is available over MCP as the `trace_error`
  tool, which takes the trace text in `trace` and reports `source: "mcp"`
  unless the caller names one.

- **One selector parser answers for every `FILE:SYMBOL` address.**
  `inspect --symbol`, `trace <target>`, `check --trace` and
  `check --symbol-impact` each carried their own split, and the emptiness guard
  had drifted: `check --trace ":"` and `check --trace "src/a.ts:"` used to parse
  into empty halves and fail later with `export or member '' not found in ''`.
  They now fail up front with the format diagnosis. The exit code is 2 either
  way, so only the message changed. Selectors still split on the LAST colon, so
  Windows drive letters and workspace-qualified paths keep theirs, and surviving
  halves are still passed through verbatim.

- **`workspace_diagnostics[]` has a stable order.** Analysis-stage diagnostics
  (`boundaries-not-configured`, `rule-packs-not-configured`,
  `source-read-failure` and the rest of the non-walk kinds) are recorded from a
  parallel detector pool, so their arrival order followed the thread schedule:
  the same command over the same commit emitted them in a different order at
  one worker than at eight. They are now ordered by path, then kind, then
  message, at the single point every consumer reads them. The list that a
  section captured from its own discovery walk keeps its meaningful order and
  still comes first. A consumer that pinned the old positional order should key
  on `kind` and `path` instead.

- **The three analysis caches change format together, so the first run after
  upgrading is cold.** The extraction cache moves to version 289, the duplication
  token cache to 13, and the graph cache to 50. Entries in the extraction cache
  now carry the inode change time beside the modification time, record whether
  complexity was actually extracted, record what a degraded parse produced, and
  are keyed on the root-relative path with the root stored in the header. The
  graph cache keys its manifest on file content rather than modification time
  and records the project root. Relocation reuses extraction entries but rebuilds
  the graph, whose retained paths are absolute.
  Older framed blobs are refused on version; older unframed blobs report a
  decode failure and are rebuilt. Nothing about the cache
  location, the `cache` config field, or `--no-cache` changes. Two consequences
  are worth naming: extraction entries in a warm tree copied with `cp -Rp` to a
  sibling path are reused instead of reparsed. On Windows, where the inode
  change time is unavailable, the metadata-only fast path is disabled and every entry is read
  and content-hashed before it is reused.

- **Churn is measured against the commit, not the wall clock.** Recency
  weighting, ownership staleness, and the churn window used to read the system
  clock at three separate points, so `weighted_commits` drifted on every run and
  `stale_days` flipped its fixed thresholds (the 90 day half-life, owner-active,
  drift minimum file age) as the day rolled over. All three now resolve one
  reference instant per run from HEAD's committer timestamp, and the window is
  passed to git as an absolute `--after=@<epoch>` instead of a phrase git
  re-resolved against the wall clock. Two runs over one commit now produce the
  same churn-derived numbers on any machine. Set `FALLOW_CLOCK_EPOCH` to pin the
  instant explicitly; imported churn (`--churn-file`) in a non-git project has no
  commit to read and warns that it fell back to the wall clock. The future
  timestamp guards on imported events deliberately stay on the wall clock, so a
  valid event is never rejected for being newer than an old HEAD. The on-disk
  churn cache moves to version 6: it is keyed on the window duration rather than
  a resolved date, and a warm load prunes to the same cutoff a cold `git log`
  applies, so a cache minted months ago no longer reports history a fresh run
  excludes. Older caches are discarded and rebuilt on first run.

- **`fallow impact` records which gate produced a run.** The local store keeps
  the `--gate-marker` value the installed gates pass (`agent`, `pre-commit`,
  `ci`) instead of only a boolean, so the report can say where your gate runs
  come from. `fallow impact --format json` gains an optional `gate_runs` object
  with a run count per source, and the human report gains one line naming the
  non-zero sources. Both are counted over the recorded runs the store still
  holds, the same bounded window `record_count` reports, so they are a floor
  rather than a lifetime total: the store keeps a bounded number of runs and
  drops the oldest, and `gate_runs` is absent when no run in that window
  carries a gate source, which is not the same as no gate ever having run.
  Additive optional under the rule above, so `ImpactReport` stays at
  `schema_version` 2. The
  on-disk store moves to schema 7: a record written by an older fallow carries
  `gate: true`, which reads back as the `unknown` source (it was a gate run
  whose origin was never stored), and `gate: false` reads back as no gate run
  at all. The store is local, never leaves the machine, and is never written in
  CI, so these counts are run provenance and not an adoption measure. An older
  fallow reading a schema 7 store degrades the way it always has: the
  statusline reports the store as written by a newer build rather than as off.

- **`code_execute` bounds the value a snippet returns, not only the fallow JSON
  it reads.** `max_output_bytes` previously capped host-call output alone, and
  any snippet result was returned whole with `ok:true`. The serialized result is
  now measured against the same number: a larger one is refused with `ok:false`
  and reported through the additive `truncated`, `result_bytes`, and
  `result_preview` fields instead of being returned, so a snippet that used to
  hand back an entire report now fails and has to return a projection. A thrown
  error message is clamped the same way, at `max_output_bytes` or 4096 bytes,
  whichever is larger, and reported with `truncated` and `error_bytes`. A
  `fallow.all` fan-out also shares one output budget rather than giving each
  element the whole of it. The envelope keeps `schema_version`
  `mcp-code-execute/v1`: every new field is optional and appears only when it
  applies. A caller that wants the old headroom can raise `max_output_bytes`,
  which accepts up to 4000000. The `calls[]` trace is bounded too, at
  `limits.max_recorded_calls` entries: a memo hit spends no budget, so a
  snippet looping one cached call used to grow the response without limit
  while every documented limit still reported as respected. Past the bound the
  host calls still run and still return, only their trace entries are dropped,
  and the additive `calls_omitted` field reports how many.

- **A referenced `tsconfig.json` without `include` or `files` no longer claims
  every file** ([#2436](https://github.com/fallow-rs/fallow/pull/2436)). When
  fallow follows project `references`, such a config now applies only to files
  under its own directory, matching tsc's `**/*` default scope. Its `paths`
  aliases previously leaked repository-wide, so an import that only resolved
  through that leak now produces an `unresolved-import` finding, which is
  error severity by default and fails `--fail-on-issues`; a file reachable
  only through such an import may now be reported as unused. Give the
  subdirectory config an explicit `include`, or move the shared aliases to a
  config whose directory contains the importing files. Root and workspace
  configs are unaffected because the root is an ancestor of every file.

- **A strict run fails on findings the override path used to let through**
  ([#2445](https://github.com/fallow-rs/fallow/issues/2445)). When a config
  contains any per-path `overrides` entry, the exit code is decided by
  per-file severity resolution. That path never consulted import-direction
  boundary violations, and it started from unpromoted base rules, so
  `--fail-on-issues` and `--ci` could exit 0 on an error-severity
  `boundary-violation` and on every `warn`-severity finding. Both are fixed,
  which means a pipeline with overrides that passed on an earlier v3 can now
  exit 1 without any config or code change. The findings themselves are
  unchanged; only the exit code is. To keep the previous outcome, set the rule
  to `off` rather than `warn`, or drop the strict flag for that job.

- **Review brief schema 9 replaces the duplicated blast-radius list with a
  count, a capped sample, and a directory rollup.** The `audit-brief` envelope
  shared by `fallow audit --brief --format json` and `fallow review --format
  json`, and the brief digest embedded in the review walkthrough guide, used to
  carry the impact closure's affected-but-not-in-diff paths TWICE, in full:
  `graph_facts.reachable_from` was a verbatim clone of
  `impact_closure.affected_not_shown`, and neither was capped. On a
  single-file change to a mid-sized project the two lists together were more
  than half the envelope, dwarfing the judgement payload the brief exists to
  deliver.

  `graph_facts.reachable_from` is REMOVED. Stage 1 keeps `exports_added`,
  `api_width_delta`, and `boundaries_touched`; the blast radius belongs to
  Stage 3, which now owns both its magnitude and its paths. Consumers reading
  `graph_facts.reachable_from` read `impact_closure` instead.

  `impact_closure` gains three fields and caps one. `affected_count` is the
  FULL number of affected-but-not-in-diff files, computed before any capping,
  so the magnitude is never understated. `affected_not_shown` is now a capped,
  path-sorted SAMPLE of at most 10 paths: it is a prefix of the sorted set, so
  it clusters in whichever directory sorts first and must not be used to
  enumerate the blast radius or to infer its shape. `affected_by_dir` is the
  shape: the affected files rolled up by parent directory as `{ dir, count }`
  rows, heaviest directory first with the directory path breaking ties, at most
  25 rows, with `affected_by_dir_omitted` counting the lighter directories that
  did not fit. Every row's `count` is exact.

  Nothing that ranks or gates moved. The decision surface reads its blast
  metric from the uncapped engine closure, not from this envelope, so decisions,
  ranks, verdicts, and exit codes are unchanged. The human brief and the human
  walkthrough report `affected_count`, so their counts are unchanged too, and
  the human brief additionally names the heaviest directory with its exact
  share. That line routes a reader to `--format json` only when nothing was
  omitted from `affected_by_dir`; past the rollup cap it says how many of the
  remaining directories the JSON actually carries. `coordination_gap`
  is untouched and uncapped; it is not a subset of `affected_not_shown`,
  because the gap deliberately skips story and test consumers that the affected
  set counts. To reconstruct the full affected set, run
  `fallow check --impact-closure <path>` once per changed file and union the
  results: that flag seeds from a single file, so no single command reproduces
  the changeset-wide union.

  Consumers that pinned `schema_version` to 8 must accept 9.

- **Review brief schema 8 adds author actions, test adjacency, independent
  slices, and dependency decisions.** All additive. A `--walkthrough-file`
  judgment may carry `action` (`block`, `address`, `consider`, `fyi`); any
  other value is rejected with `reason: "invalid-action"`, and accepted
  judgments echo `action` next to `agent_framing`. The guide's `agent_schema`
  gains `action_vocabulary` and `concern_vocabulary`; `direction.units[]`
  gains an optional `test_adjacency` (`none`, `untouched`, `changed`);
  `partition` gains `independent_slices`; `deltas` gains `dependency_added`
  and `dependency_major_bumped`. The `dependency` decision category, reserved
  since the decision surface shipped, now produces decisions from changed
  `package.json` manifests. Consumers that pinned `schema_version` to 7 must
  accept 8.

- **Source files a built-in discovery ignore pattern removed now report a
  diagnostic** ([#2638](https://github.com/fallow-rs/fallow/issues/2638)).
  Source discovery has always dropped files matching its built-in ignore
  patterns (`**/dist/**`, `**/build/**`, `**/coverage/**`, the four
  minified-bundle globs); the drop was silent, so pointing fallow at a
  directory one of them matches returned a clean report with exit 0. The walk
  now records one `excluded-by-default-ignore` workspace diagnostic per
  excluding pattern, carrying that pattern verbatim, an exact `file_count`, a
  `directory_count`, and a `path` anchored at the directory that lost the most
  files. Discovery is unchanged: no file is newly analyzed and no file is newly
  skipped. Files a project's own `ignorePatterns` also matched are attributed to
  nothing, a gitignored tree never reaches the walk and counts zero, and
  `**/node_modules/**` is never reported at all. The kind is not a
  `source_never_analyzed` kind, so no finding gains a `reachability_caveat` and
  no `fallow fix` action is withheld. Human output is unchanged unless
  `--explain-skipped` is passed, with one exception: a run that discovered no
  source files at all while a built-in pattern excluded some now says so on
  stderr, on `check`, `dead-code`, `audit` and the default run, because the
  alternative is a green result that is misleading. That line states the two
  facts it measured and joins them with a period ("No source files were
  analyzed. The built-in ignore pattern `**/build/**` excluded 3 files"); it
  does not claim the pattern is the reason the run was empty, because
  `--production` and the other skips can empty a file list on their own. SARIF,
  CodeClimate, compact, and badge output carry no workspace diagnostic and are
  unchanged. The new kind moves no `schema_version` under the open-set exception
  for `workspace_diagnostics[].kind` documented above.

- **Hidden directories that hold source files now report a diagnostic**
  ([#461](https://github.com/fallow-rs/fallow/issues/461)). Source discovery has
  always skipped dot-prefixed directories outside a small convention allowlist;
  the skip was silent. It now records a `skipped-source-dotdir` workspace
  diagnostic and one aggregated stderr note when a skipped directory holds
  source files the project has not excluded. Traversal is unchanged: no
  directory is newly analyzed and no directory is newly skipped. "Not excluded"
  matches what the run would analyze, so a directory whose contents are
  gitignored, matched by `ignorePatterns`, or (on a `--production` run) excluded
  as test or story files stays silent, and so do generated tool output
  directories and non-git VCS metadata. The new kind moves no `schema_version`
  under the open-set exception for `workspace_diagnostics[].kind` documented
  above; a consumer validating against a pinned older schema copy should move
  to the schema shipped with the version it runs.

- **`.pnpm` is no longer auto-scoped into discovery from a `package.json`
  script** ([#461](https://github.com/fallow-rs/fallow/issues/461)). `.pnpm` was
  missing from the script-scope denylist while `.pnpm-store` was present, so a
  script argument such as `node .pnpm/tool/bin.mjs` pulled that directory into
  source discovery. It now behaves like every other package-manager state
  directory: the files leave the discovered set, and no `skipped-source-dotdir`
  advisory replaces them, because a denylisted directory never earns one. A
  repository that tracks source under a root-level `.pnpm` and relied on the
  script reference to reach it loses those findings; move the files or add the
  directory to a traversed location. The `node_modules/.pnpm` shape is not
  observable either way, since the built-in `node_modules` exclusion already
  dominates there.

- **A `package.json` script reference now scopes the directory it names, not
  the name anywhere in the tree**
  ([#461](https://github.com/fallow-rs/fallow/issues/461)). A script argument
  such as `node .tools/.private/build.mjs` made `.tools` and `.private`
  traversable wherever either name appeared, so an unrelated
  `packages/web/.private` was pulled into discovery by a script that never
  referenced it. The inferred scope is now the exact root-relative path the
  script named, so the example admits `.tools` and `.tools/.private` only.
  Directories that were reached only through the name match leave the
  discovered set and their findings disappear. They earn a
  `skipped-source-dotdir` advisory when they hold source, so the loss is
  visible rather than silent. Plugin-contributed conventions are unaffected: a
  framework's `.client` and `.server` still match at any depth under the
  package that activates the plugin, because the plugin declares a convention
  rather than a location.

- **More generated-output and VCS directories are excluded from script
  scoping** ([#461](https://github.com/fallow-rs/fallow/issues/461)). The
  script-scope denylist covered the build caches of one framework generation.
  It now also covers `.angular`, `.astro`, `.contentlayer`, `.expo`,
  `.react-router`, `.rollup.cache`, `.sst`, `.swc`, `.tanstack`, `.velite`,
  `.vinxi`, `.wrangler`, `.wxt`, `.yalc`, and the `.hg`, `.jj`, and `.svn`
  metadata trees. A script argument pointing into one of these no longer pulls
  it into source discovery, so machine-written output and VCS object stores
  stay out of the analyzed set. A repository that kept first-party source under
  one of these names and relied on a script reference to reach it loses those
  findings; move the files to a traversed location.

- **Trace evidence is reachability-aware and lane-complete**
  ([#2390](https://github.com/fallow-rs/fallow/issues/2390)). This supersedes
  the remaining #2371 caveats documented below. `is_used` now agrees with the
  dead-code verdict when every direct consumer is unreachable, declaration
  merges use the declaration group that dead-code credits, and a star-export
  collision returns ambiguity rather than ordinary unused or not-found output.
  The trace root adds optional `direct_references_by_namespace` evidence so
  type and value references can be inspected without changing the existing
  selected `namespace` field or silently redefining its totals. Type-aware
  proof no longer treats an import or re-export declaration as a read and no
  longer suppresses a finding with unreachable-only, re-export-only, or
  different-declaration evidence. These are additive fields and evidence
  corrections on the unversioned trace contract, so no trace schema version
  moves. Consumers should prefer the explicit ambiguity payload and per-lane
  evidence over deriving a verdict from `direct_references.length`.

- **Programmatic workspace diagnostics belong to the run that produced them**
  ([#2392](https://github.com/fallow-rs/fallow/issues/2392),
  [#2394](https://github.com/fallow-rs/fallow/issues/2394), and
  [#2396](https://github.com/fallow-rs/fallow/issues/2396)). This supersedes
  the #2366 process-registry behavior described above. Analysis-owning
  dead-code, check, health, dupes, and security envelopes carry only their
  stage-owned root-relative diagnostics. Project-info, workspace-listing, and
  duplicate-list surfaces do not inherit analysis-stage diagnostics from an
  earlier call in the same process. Security full, summary, survivors, and
  blind-spots roots add an optional `workspace_diagnostics` field that remains
  absent when empty. The new Bun lockfile and shadowed-resolution diagnostic
  kinds are additive. No schema version moves under the additive-field policy.

- **Export traces first exposed the namespace that credits the declaration**
  ([#2371](https://github.com/fallow-rs/fallow/issues/2371)). This historical
  change made `trace_export` fall back from the preferred lane to the other
  lane when both resolved to the same binding, and added the optional
  `namespace` and `owner_namespace` fields across trace, inspect, CLI, and MCP
  output. The initial implementation did not yet reconcile unreachable
  consumers, every legal cross-lane declaration merge, or checker evidence
  with the graph verdict. Those limitations are superseded by #2390 above:
  only reachable references credit `is_used`, legal declaration merges share
  their declaration group, and type-aware proof cannot refute a finding with
  unreachable-only, re-export-only, or different-declaration evidence.
  `semantic.target.namespace` continues to name the declaration lane and can
  differ from the root crediting `namespace`. The added fields remain optional
  in the schema, and the unversioned trace roots remain compatible.

- **Type-aware semantic gap reasons use wire protocol 7 and semantic schema version 3.** The new required `SemanticOmission.reason_code` enum value is not ignorable by a protocol 6 reader, so exact-version companion pairing now uses wire protocol 7. Every versioned JSON root that can embed that contract also bumps: audit 9 to 10, coverage analyze 1 to 2, health 10 to 11, duplication CLI 8 to 9 and programmatic 2 to 3, dead-code flat and grouped 8 to 9, per-project and cross-repo impact 1 to 2, security full and summary 7 to 8, combined 10 to 11, and audit brief 6 to 7. Unversioned trace and inspect roots are unchanged.

- **Export traces select namespaces deterministically.** `trace_export` now selects the Value namespace when a module exposes the same name as both a value and a type, falling back to Type only when no Value binding exists. Produced JSON carries the additive `namespace` field (`"value"` or `"type"`), including through the API and MCP adapters; the schema keeps it optional so consumers can still accept older payloads. Previously selection could depend on reference counts and named re-export declaration order, and consumers could not identify which namespace was traced.

- **Declarations behind an ambiguous `export *` are no longer reported as unused** ([#2263](https://github.com/fallow-rs/fallow/issues/2263)). When two different star sources of a barrel supply the same name, the barrel exports nothing under that name (ECMA-262 ResolveExport returns `ambiguous`), so no importer can credit the declarations that feed it. Those declarations used to appear as unused-export and unused-type findings in every source file that declared the colliding name, which pointed at the sources for a mistake that lives in the barrel. They are now suppressed until the collision is resolved, in both the value namespace and the type namespace, the latter covering `export type *` sources colliding over `class` or `enum` declarations. Sibling exports in the same files are unaffected. Scripts that counted unused-export findings on such a project will see fewer, and fixing the barrel makes any genuinely dead contributor reappear. `fallow trace FILE:NAME` reports the collision itself through the additive optional `star_export_ambiguity` field ([#2262](https://github.com/fallow-rs/fallow/issues/2262)), which names the colliding origins and the namespaces involved.

- **Workspace public-API entry points require `publicPackages`** ([#2210](https://github.com/fallow-rs/fallow/pull/2210)). Every workspace package's exports-mapped modules used to become public-API entry points unconditionally, which suppressed unused-export, unused-member, and unrendered-component findings across whole monorepos. Entry-point selection now honors the existing `publicPackages` list, matching the public workspace-root selection that already honored it: with the default empty list, workspace exports are no longer implicit public API, and previously suppressed findings can start firing. Monorepos that publish workspace packages should list them in `publicPackages` to keep the suppression.

- **Health threshold-override rows are per dimension and carry a required `dimension` field** ([#2163](https://github.com/fallow-rs/fallow/issues/2163)). The health JSON schema is version 10, and the bare combined envelope that embeds the health report moves to version 10 with it; the audit envelope does not embed the health contract and is unchanged. One configured `health.thresholdOverrides` entry emits one `threshold_overrides[]` row per dimension it participates in (`complexity` for the structural ceilings `maxCyclomatic`, `maxCognitive` and `maxUnitSize`, `crap` for the `maxCrap` ceiling), so rows on the previously-documented path are no longer byte-identical and the additive-field exemption does not apply. `status` gains the value `insufficient` for an override that raises a ceiling the matched code still exceeds; that case previously emitted no row. The optional `outstanding[]` array lists the dimensions the matched unit still breaches after the override applied. Matched rows also carry the unit's optional `line` and `col` so two units that share a name in one file stay distinct, and one entry that configures only `maxUnitSize` now emits a `complexity` row when it matches instead of none; an entry scoped with `functions: ["<component>"]` reaches the synthetic Angular rollup and is reported as matched rather than `no_match`. Consumers that counted rows to count configured overrides should group on `override_index` instead. The human report's surviving-dimension suffix reads `(still breaches: ...)` where it previously read `(finding still fires on: ...)`, because a `maxUnitSize` breach keeps a unit in the large-function list without emitting a finding.

- **Duplication findings use spread-aware order and normalized fingerprints** ([#2155](https://github.com/fallow-rs/fallow/issues/2155)). Clone groups are ranked by size, occurrences, and capped directory or line spread. Formatting-only edits no longer change clone fingerprints. Existing saved baselines keep matching through a legacy raw-fingerprint fallback, while newly saved baselines use normalized keys. CLI duplication uses schema version 8 and programmatic duplication uses its independent version 2 because clone-group findings now require `spread` and `duplicated_tokens` now counts redundant copies; near-miss groups add optional `similarity`. Combined and audit output advance independently when an embedded contract changes.

- **CI-facing formats emit repository-root-relative paths when `--root` is a subdirectory** ([#1808](https://github.com/fallow-rs/fallow/pull/1808)). `codeclimate`, `review-github`, and `review-gitlab` used to address files relative to `--root`, which GitLab's Code Quality widget and the GitHub/GitLab review APIs rejected for package-subdirectory roots; they now rebase onto the git toplevel like `github-annotations`. Single-package repositories are unaffected. Wrapper scripts that prepended the offset themselves should drop that step, or pass `--report-path-prefix ''` to restore the old output. `--annotations-path-prefix` was renamed to `--report-path-prefix` with the old name kept as an alias.

## Notable behavior changes within v2

These are documented for the rare CI script that depended on the old behavior. None require a config migration.

- **`fallow health --hotspots --format json` outside a git repository now exits 0** (was exit 2). Missing git history is treated as unavailable hotspot data: the `hotspots` array is omitted (empty) and `hotspot_summary` is omitted, with a non-fatal `note: hotspot analysis skipped: no git repository found at project root` on stderr (suppressed by `--quiet`). Combined-mode `--format json` always emits exactly one JSON document on stdout regardless of git state. CI scripts that asserted exit 2 to detect "no git repo" should inspect `hotspot_summary` (absent when not analyzed, present otherwise) instead. Fixed in [#297](https://github.com/fallow-rs/fallow/pull/297).
- **`--coverage` paths now resolve relative to `--root`; `--coverage-root` must be absolute**. `fallow health --coverage relative/path.json --root sub-project/` (and the same flags on `fallow audit` or bare `fallow`) used to look for `cwd/relative/path.json`, breaking monorepo CI runs that invoke fallow from the workspace root with a sub-project `--root`. Relative `--coverage` paths now resolve under `--root` like every other project input, so the same invocation finds `sub-project/relative/path.json`. `--coverage-root` is different: it strips a prefix from paths inside the coverage data, so relative values such as `src` are rejected. Pass the absolute source prefix from the machine that generated coverage, for example `/home/runner/work/myapp`.
- **Config-sourced glob patterns are validated at load time** ([#463](https://github.com/fallow-rs/fallow/issues/463)). User-supplied globs in `entry`, `ignorePatterns`, `dynamicallyLoaded`, `duplicates.ignore`, `health.ignore`, `overrides[].files`, `ignoreExports[].file`, `ignoreCatalogReferences[].consumer`, `boundaries.zones[].patterns`, and `boundaries.coverage.allowUnmatched` must be relative to the project root, may not contain `..` traversal segments, and must be syntactically valid glob patterns. Invalid patterns previously no-op'd (silently dropped at three call sites in `entry_points.rs`) or warn-and-skipped (everywhere else); they now fail at config load with exit code 2 and a message naming every offending field + pattern. Configs that silently ran with broken patterns must fix them to upgrade.
- **Invalid plugin regex patterns are hard errors** ([#513](https://github.com/fallow-rs/fallow/issues/513)). Regexes supplied by external plugin configs, including path exclusion regexes, segment exclusion regexes, and used-export path regexes, must use Rust-compatible regex syntax. Unsupported constructs such as JavaScript lookahead or lookbehind now fail plugin loading with exit code 2 instead of being skipped during matching. Plugin authors should rewrite those patterns as Rust-compatible regexes or remove the unsupported rule.

## Config format migration

The `fallow migrate` command helps migrate between config formats. When breaking config changes happen in a major version, `migrate` will be updated to handle the transition.
