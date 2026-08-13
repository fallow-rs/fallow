# Backwards Compatibility Policy

Starting with v1.0, fallow follows [semantic versioning](https://semver.org/).

## What is stable

These interfaces are covered by semver , breaking changes only happen in major version bumps:

### Configuration format

- **Config file names**: `.fallowrc.json`, `.fallowrc.jsonc`, `fallow.toml`, `.fallow.toml`
- **All documented config fields**: `extends`, `ignorePatterns`, `ignoreFindings`, `rules`, `overrides`, `entry`, `ignoreDependencies`, `ignoreExports`, `ignoreExportsUsedInFile`, `ignoreDecorators`, `unusedComponentProps` (with `ignorePattern`), `includeEntryExports`, `autoImports`, `duplicates`, `audit`, `cache`, `fix`, `production` (boolean form `production: true` or per-analysis form `production: { deadCode, health, dupes }`), `framework`, `workspaces`, `plugins`, `rulePacks`, `boundaries` (including `boundaries.preset`, `boundaries.coverage`, and `boundaries.calls`)
- **Duplication triage fields**: `duplicates.near` enables opt-in function-level near-miss detection. `duplicates.ignoredClones` accepts normalized `<fingerprint>:<instance_count>` keys and resurfaces a reviewed group when its token content or occurrence count changes.
- **Rule names and severity values**: `unused-files`, `unused-exports`, etc. with `error`/`warn`/`off`
- **Extends and overrides semantics**: merge behavior, glob matching, override precedence, `npm:` prefix resolution, `https://` URL resolution
- **`ignoreFindings` semantics**: patterns are validated as project-root-relative globs, leading `!` patterns keep reportable exceptions, source-owned dead-code findings with multiple owners are hidden only when every owner matches, and architecture, policy, suppression-hygiene, and framework-correctness findings remain visible, as do manifest-owned findings that no source file owns (unused dependencies, unused dev and optional dependencies, catalog entries, and dependency overrides). An unlisted dependency is source-owned by its import sites, so it is hidden only when every import site matches. The opt-in React health signals are source-owned too, so they are hidden as well: a thin wrapper by its component file, a duplicate prop shape by the file of the component the finding is emitted for (siblings keep their own findings and still list the hidden component in `sharing_components`), and a prop-drilling chain only when every hop file matches. Security candidates and their blind-spot metadata (`fallow security` findings, unresolved-callee diagnostics, and unresolved-edge counts) are never hidden by `ignoreFindings`: a path glob must not silence a leak candidate or turn an unresolved blind spot into a clean bill
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
- **Schema version**: each output envelope versions independently from the tool and from sibling envelopes. The affected envelope is bumped when an EXISTING wire field is renamed, removed, or its type changes, when a value is added to an existing enum-valued required field, OR when a `required` field is added to a previously-documented finding. An envelope that embeds the changed contract bumps too; unrelated envelopes do not. Additive optional fields (new fields with `#[serde(skip_serializing_if = ...)]` that are absent on the wire by default, or new finding types added to brand-new issue-type arrays) do NOT bump `schema_version`: existing consumers see a byte-identical wire shape on the unchanged path. Exact envelope versions are encoded as numeric `const` values in `docs/output-schema.json`; a shared CLI/programmatic shape with separate version lineages encodes the closed numeric set. The generated TypeScript contract derives its literal types from those schema definitions.
- **Audit styling fields**: `fallow audit` includes styling analytics by default. The nested health block may contain `css_analytics`, `styling_health`, and `styling_findings` for CSS, Sass/Less, CSS Modules, Tailwind/shadcn/CVA, StyleX/PandaCSS, vanilla-extract, styled-components, and Emotion projects. Under `gate: new-only`, styling findings carry the same optional `introduced` marker as other findings and the attribution block includes `styling_introduced` / `styling_inherited` totals. These fields are additive JSON output, and styling findings are verdict-neutral unless the corresponding rule is configured to `error`; they do not require a `schema_version` bump under the additive-field policy. Snapshot-diffing consumers can set `audit.css: false` or pass `--no-css` to suppress styling entirely.
- **Document-root structure**: every object-shaped `--format json` envelope covered by the typed root schema (`FallowOutput`) carries a top-level `kind` discriminator. Consumers should branch on `kind` instead of probing for unique field presence. The authoritative set of typed root kinds lives in `docs/output-schema.json`; the factual list below is checked against that schema manifest:
  <!-- fallow-output-kind-list:start -->
  `audit`, `explain`, `inspect_target`, `trace`, `review-envelope`, `review-reconcile`, `coverage-setup`, `coverage-analyze`, `list-boundaries`, `list-workspaces`, `health`, `dupes`, `dead-code-grouped`, `impact`, `impact-cross-repo`, `security`, `security-survivors`, `security-blind-spots`, `dead-code`, `combined`, `feature-flags`, `audit-brief`, `decision-surface`, `review-walkthrough-guide`, `review-walkthrough-validation`, `suppression-inventory`, `type-aware-status`
  <!-- fallow-output-kind-list:end -->
  Tagged root envelopes are now the only supported object-shaped JSON contract. The CLI `check` command is a legacy alias for `dead-code`; new JSON discriminators use the canonical `dead-code` name. `CodeClimateOutput` stays as a sibling root branch because the Code Climate / GitLab Code Quality spec requires a bare JSON array at the root; discriminate it by checking whether the document root is an array. Helper/spec JSON roots outside `FallowOutput`, such as `fix`, `fallow config`, non-boundary `fallow list` modes, SARIF, CodeClimate, telemetry, and baseline/config files written by fallow, are not part of this envelope contract.
- **Security survivor schema**: `security-survivors` uses schema version `2`; `summary.unverdicted` is required and reports candidates without matching verifier verdicts.

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

- **Subcommands**: `dead-code` (legacy alias: `check`), `dupes`, `health`, `audit`, `security`, `explain`, `fix`, `watch`, `init`, `hooks`, `setup-hooks`, `migrate`, `list`, `schema`, `config-schema`, `plugin-schema`, `config`, `coverage`, `license`, `ci`. `security` is opt-in (the `security-client-server-leak` rule defaults to `off`); its findings never appear under bare `fallow` or `audit`.
- **`coverage` subcommands**: `setup`, `analyze`, `upload-source-maps`, `upload-inventory`. `analyze` accepts `--runtime-coverage <path>` for local mode and `--cloud` / `--runtime-coverage-cloud` (or `FALLOW_RUNTIME_COVERAGE_SOURCE=cloud`) for explicit cloud-pull; `FALLOW_API_KEY` alone never selects cloud mode.
- **`license` subcommands**: `activate`, `status`, `refresh`, `deactivate`, `trial`. JWT verification is offline-only; `activate` and `refresh` are the only network-touching operations.
- **Default behavior**: bare `fallow` (no subcommand) runs dead-code + dupes + health combined
- **Exit codes**: 0 (success/no errors), 1 (issues with error severity found), 2 (runtime error). `fallow audit` defaults to `--gate new-only`, so inherited error-severity findings in changed files can be reported with exit 0; use `--gate all` to fail on every finding in changed files. `fallow security --gate new` and `fallow security --gate newly-reachable` add exit code **8**, dedicated to a security candidate matching the selected gate mode (changed-line candidate or newly entry-reachable candidate). A gate that cannot compute its required diff or base tree exits 2, not 8. The code is stable so pipelines can pin it (for example GitLab `allow_failure: exit_codes: [8]`). The official GitHub Action exposes the same gate through `security-gate`, and the GitLab template exposes it through `FALLOW_SECURITY_GATE`.
- **Global flags**: `--format`, `--config`, `--workspace`, `--production`, `--no-production` (force production mode off, overriding a project config's `production: true`; conflicts with `--production`), `--baseline`, `--save-baseline`, `--baseline-mode` (health baseline matching: `count` per file and category, the default, or `identity` per function identity for strict regression gates; an identity comparison requires a baseline saved with `--baseline-mode identity`; a save that omits the flag refuses to overwrite a baseline carrying identity buckets, and an explicit `--baseline-mode count` downgrades it on purpose), `--no-cache`, `--threads`, `--changed-since` (alias: `--base`), `--churn-file` (import a `fallow-churn/v1` JSON change-history file for hotspots/ownership/targets on non-git VCS), `--performance`, `--explain`, `--ci`, `--fail-on-issues`, `--sarif-file`, `--output-file` (alias: `-o`; write the report to a file instead of stdout, for any `--format`), `--fail-on-regression`, `--tolerance`, `--regression-baseline`, `--save-regression-baseline`, `--summary`, `--group-by` (owner, directory, package, section), `--include-entry-exports`, `--max-file-size` (skip source files larger than N megabytes at discovery, default 5, `0` disables; declaration files are always analyzed), `--dupes-mode`, `--dupes-near`, `--dupes-threshold`, `--dupes-min-tokens`, `--dupes-min-lines`, `--dupes-min-occurrences`, `--dupes-skip-local`, `--dupes-cross-language`, `--dupes-ignore-imports`, `--dupes-no-ignore-imports` (count module wiring in combined mode; opt out of the default exclusion)
- **Per-analysis production flags**: `--production-dead-code`, `--production-health`, `--production-dupes` (bare combined mode and `fallow audit`)
- **Bare command flags**: `--only`, `--skip` (select which analyses to run), `--coverage` (Istanbul coverage data for the embedded health analysis), `--coverage-root` (absolute coverage-data prefix for CI rebasing), `--score` (health score in combined mode), `--trend` (compare against snapshot), `--save-snapshot` (save vital signs for trend tracking)
- **Health flags**: `--score` (project health score 0-100 with letter grade), `--min-score` (CI quality gate), `--max-cyclomatic` / `--max-cognitive` / `--max-crap` (per-function complexity thresholds; CRAP combines complexity with coverage), `--targets` (refactoring recommendations), `--effort` (filter targets by effort level: low/medium/high), `--coverage-gaps` (static test coverage gaps), `--coverage` (Istanbul coverage data for accurate CRAP scores), `--coverage-root` (absolute coverage-data prefix for CI rebasing), `--save-snapshot` (saves vital signs snapshot for trend tracking), `--trend` (compare against most recent snapshot)
- **Audit flags**: `--gate <new-only|all>` (controls whether only introduced findings or all findings affect the verdict), `--max-crap` (forwarded to the health sub-analysis; mirrors `health.maxCrap` in config), `--coverage` (Istanbul coverage data for accurate CRAP scores), `--coverage-root` (absolute coverage-data prefix for CI rebasing), `--no-css` (disable audit styling analytics), `--css-deep` (force deep styling analytics on when config disables it), `--no-css-deep` (skip project-wide styling reachability while keeping local styling checks)
- **Security flags and subcommands**: `--gate <new|newly-reachable>` (security candidate regression gate, exit code 8 on a matching candidate), `--surface` (include attack-surface inventory), `--file <path>` (candidate scope, also accepted after `security blind-spots`), `--runtime-coverage <path>` (runtime ranking signal), `--min-invocations-hot <n>` (runtime hot-path threshold), `security survivors --candidates <file> --verdicts <file> --require-verdict-for-each-candidate` (render verifier-retained survivor candidates, with optional complete-verdict gate), `security blind-spots` (group unresolved callee blind spots)
- **Init flags**: `--toml`, `--hooks` (scaffold pre-commit git hook), `--branch` (fallback base branch/ref for the hook when no upstream is set)
- **Hooks command**: `hooks install|uninstall --target <git|agent>` manages Git pre-commit hooks and agent gates. `setup-hooks` remains supported as the legacy agent-hook command.
- **Environment variables**: `FALLOW_FORMAT`, `FALLOW_QUIET`, `FALLOW_BIN`, `FALLOW_TIMEOUT_SECS`, `FALLOW_EXTENDS_TIMEOUT_SECS`, `FALLOW_COVERAGE`, `FALLOW_COVERAGE_ROOT`, `FALLOW_CACHE_DIR`, `FALLOW_API_URL`, `FALLOW_API_KEY`, `FALLOW_CA_BUNDLE`, `FALLOW_PRODUCTION`, `FALLOW_PRODUCTION_DEAD_CODE`, `FALLOW_PRODUCTION_HEALTH`, `FALLOW_PRODUCTION_DUPES`, `FALLOW_REVIEW_GUIDANCE`, `FALLOW_REVIEW_ID`, `FALLOW_SUMMARY_SCOPE`, `FALLOW_AUDIT_CACHE_MAX_AGE_DAYS`, `FALLOW_UPDATE_CHECK`, `FALLOW_MAX_FILE_SIZE` (per-file size limit in megabytes, mirrors `--max-file-size`; `0` disables), `FALLOW_SUGGESTIONS` (set to `off`/`0`/`false`/`no`/`disabled` to suppress the `next_steps[]` array in JSON output and the human `Next:` line; default on)
- **CI comment formats**: `pr-comment-github`, `pr-comment-gitlab`, `review-github`, and `review-gitlab` are stable machine-oriented markdown/envelope formats for bundled CI integrations. Wording, grouping, and markdown presentation can improve in minor releases, but marker comments, review fingerprints, and documented control variables such as `FALLOW_SUMMARY_SCOPE`, `FALLOW_REVIEW_GUIDANCE`, `FALLOW_REVIEW_ID`, `FALLOW_BOT_LOGIN`, and `FALLOW_MAX_COMMENTS` remain compatible. Scoped envelopes carry `meta.review_id`, and generated summary, inline, and resolution bodies repeat the exact scope marker so reconciliation cannot cross review jobs; unscoped jobs see only unscoped bodies.
- **Health config fields**: `health.coverage` and `health.coverageRoot` are stable fallbacks for standalone health and bare combined mode when the matching CLI flag and env var are omitted.
- **Generated hook-script env vars**: `FALLOW_GATE_MIN_VERSION` (consumed by
  the generated `fallow-gate.sh` in the target project's Claude hooks
  directory; written by `fallow hooks install --target agent` or
  `fallow setup-hooks`; controls the minimum fallow version the gate accepts;
  the default is hand-bumped each release, with the generated
  `crates/cli/src/setup_hooks/fallow-gate.sh` as the source of truth; empty
  string disables) and `FALLOW_GATE_DEBUG` (any
  non-empty value makes the same script log a stderr notice when it skips a
  command it does not classify as a `git commit` or `git push`)

### External plugin format

- **Plugin file structure**: as documented in `docs/plugin-authoring.md`
- **Detection types**: `dependency`, `fileExists`, `all`, `any`

### Type-aware protocol

- **Stable starting point**: wire protocol version 6 is the first stable
  contract between Fallow and the optional `fallow-type-aware` companion.
- **Exact-version pairing**: the native binary and companion package must have
  the same Fallow version. The backend version and supported operations come
  from `crates/api/type-aware-protocol.json`.
- **Evolution**: additive response fields may be introduced when older readers
  can ignore them. Removing or changing an operation, envelope, or required
  field requires a new wire protocol version and a documented compatibility
  path.
- **Pre-stable protocols**: development-only protocols before version 6 are
  rejected and are not part of the compatibility guarantee.

## What may change in minor/patch versions

These are explicitly **not** covered by the stability guarantee:

- **New fields** may be added to config, JSON output, or plugin format (additive changes)
- **New issue types** may be added
- **New plugins** may be added to the built-in set
- **Detection accuracy**: false positive/negative rates may improve
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

- **Export traces select namespaces deterministically.** `trace_export` now selects the Value namespace when a module exposes the same name as both a value and a type, falling back to Type only when no Value binding exists. Its JSON object carries the required additive `namespace` field (`"value"` or `"type"`), including through the API and MCP adapters. Previously selection could depend on reference counts and named re-export declaration order, and consumers could not identify which namespace was traced.

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
