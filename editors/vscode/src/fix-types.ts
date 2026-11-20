/**
 * Types for `fallow fix --format json` output. This command's shape is not
 * yet covered by `docs/output-schema.json`, so these stay hand-written. The
 * runtime `FixAction` here is distinct from the schema's `FixAction` in
 * `generated/output-contract.d.ts` (which describes a SUGGESTION inside an
 * `issue.actions[]` array). They share the name historically but represent
 * different concepts.
 */

export interface FixAction {
  readonly type: string;
  readonly path?: string;
  readonly line?: number;
  readonly name?: string;
  readonly package?: string;
  readonly location?: string;
  readonly file?: string;
  /**
   * Reverse-fingerprint identifier set on `applied: true` entries that
   * created new files. Populated by `fallow fix`'s missing-config
   * fallback for duplicate-export rules: the array always contains
   * `.fallowrc.json` (the path the create-fallback writes).
   */
  readonly created_files?: ReadonlyArray<string>;
  /**
   * Unified-diff preview of the proposed config write, present on
   * `dry_run: true` entries for `add-to-config` actions. Hand-rolled
   * `+`-prefix output for the create-fallback case (BEFORE is empty),
   * `similar::TextDiff` unified diff for the edit case.
   */
  readonly proposed_diff?: string;
  /**
   * Set to `true` on entries emitted in `--dry-run` mode.
   */
  readonly dry_run?: boolean;
  /**
   * Set to `true` when the applier intentionally skipped this entry
   * (see `skip_reason`).
   */
  readonly skipped?: boolean;
  /**
   * Reason the entry was skipped. Known values:
   * - `hardcoded_consumers` (catalog entry has workspace consumers that
   *   still pin a hardcoded version).
   * - `missing_config` (legacy reason, pre-#332, when no config existed
   *   and the writer declined to create one).
   * - `monorepo_subpackage` (the duplicate-export config-add path
   *   refused to create `.fallowrc.json` inside a monorepo subpackage).
   * - `no_create_config` (`--no-create-config` was passed and no fallow
   *   config exists).
   * - `content_changed` (#454: file's xxh3 content hash at fix time
   *   differs from the hash captured during analysis; applying offsets
   *   would land on bytes the analysis never saw). Re-run `fallow fix`
   *   to refresh the analysis.
   * - `mixed_line_endings` (#475: file mixes CRLF and bare-LF line
   *   endings; not self-healing, normalize with `dos2unix` first).
   * - `low_confidence_off_graph` (#602: export lives in a test, mock, or
   *   fixture directory whose consumers fallow's graph cannot see;
   *   removal withheld. Still reported by `fallow check`).
   * - `low_confidence_unresolved_imports` (#602: the file has an
   *   unresolved import, so its usage graph is incomplete; export removal
   *   withheld).
   * - `low_confidence_incomplete_analysis` (the finding carries
   *   `reachability_caveats`: a source file was skipped, unreadable, or
   *   did not parse cleanly, so the import that would have credited the
   *   export or the package may never have been seen. Both the export
   *   removal and the `remove-dependency` write are withheld).
   * - `kept_re_export` (#4636: a `remove_export` entry whose removal is
   *   withheld because a re-export that the run keeps still names the
   *   export. Counted by `skipped_kept_re_exports`).
   * - `shared_declaration_in_use` (#4659: a `remove_export` entry whose
   *   removal is withheld because the same `export` keyword also exports a
   *   name that is in use, as in `export const { a, b } = x;`. Counted by
   *   `skipped_shared_declarations`).
   * - `generated_source` (#4912: the file is generated code, and its
   *   generator writes it again, so the export removal is withheld).
   *
   * The three `low_confidence_*` reasons, `kept_re_export`,
   * `shared_declaration_in_use` and `generated_source` are
   * INTENTIONAL skips: they do NOT cause a non-zero exit code (unlike
   * `content_changed` / `mixed_line_endings`).
   */
  readonly skip_reason?: string;
  /**
   * Reachability caveat tokens copied from the finding, present on
   * `skip_reason: "low_confidence_incomplete_analysis"` entries so a
   * consumer gates on the marker instead of parsing the reason string.
   * Same open token set as the dead-code `reachability_caveats` array.
   */
  readonly reachability_caveats?: ReadonlyArray<string>;
  /**
   * Workspace root path emitted on `skip_reason: "monorepo_subpackage"`
   * entries so consumers can point the user at `fallow init` at the
   * monorepo root. Relative to the analysis root.
   */
  readonly workspace_root?: string;
}

export interface FallowFixResult {
  readonly dry_run: boolean;
  readonly fixes: ReadonlyArray<FixAction>;
  readonly total_fixed: number;
  /**
   * Count of fixer-logic skips (catalog `hardcoded_consumers`,
   * `multi_document_yaml`, `not_found`, `not_empty`,
   * `unsupported_yaml_style`, `unsafe_edit`, `monorepo_subpackage`,
   * `no_create_config`). Semantics unchanged since pre-#454; disjoint
   * from `skipped_content_changed`.
   */
  readonly skipped?: number;
  /**
   * Count of files skipped because their xxh3 content hash at fix time
   * differed from the hash captured during analysis (#454). Always
   * present in the envelope; defaults to 0. A non-zero value means
   * `fallow fix` exited 2; consumers re-run after refreshing analysis.
   */
  readonly skipped_content_changed?: number;
  /**
   * Count of files skipped because they mix CRLF and bare-LF line
   * endings (#475). Always present; defaults to 0. A non-zero value
   * means `fallow fix` exited 2; the file must be normalized first.
   */
  readonly skipped_mixed_line_endings?: number;
  /**
   * Count of files whose unused-export removals were withheld as low
   * confidence because consumers may be invisible to static analysis
   * (#602): the file is under a test/mock/fixture directory, or it has
   * an unresolved import. Always present; defaults to 0. Unlike the two
   * counters above, a non-zero value does NOT change the exit code (it
   * is an intentional, conservative skip). The exports stay reported by
   * `fallow check`; the per-entry `skip_reason` distinguishes the three
   * `low_confidence_*` causes, the third being a finding whose verdict
   * rests on a source file the run did not fully analyze.
   */
  readonly skipped_low_confidence_exports?: number;
  /**
   * Count of declared packages whose `remove-dependency` write was
   * withheld because the finding carried `reachability_caveats`: a source
   * file was skipped, unreadable, or did not parse cleanly, so the import
   * that would have credited the package may never have been seen. Always present; defaults to 0.
   * Counted apart from `skipped_low_confidence_exports` so neither name
   * lies about what it counts, and likewise does NOT change the exit
   * code.
   */
  readonly skipped_low_confidence_dependencies?: number;
  /**
   * Count of `remove_export` entries withheld with
   * `skip_reason: "kept_re_export"`: a re-export that the run keeps still
   * names the export, so its removal would break that re-export. Counts
   * exports, not files. Always present; defaults to 0. Does NOT change the
   * exit code.
   */
  readonly skipped_kept_re_exports?: number;
  /**
   * Count of `remove_export` entries withheld with
   * `skip_reason: "shared_declaration_in_use"`: the same `export` keyword
   * also exports a name that is in use, so the removal of the keyword would
   * remove that export too. Counts exports, not files. Always present;
   * defaults to 0. Does NOT change the exit code.
   */
  readonly skipped_shared_declarations?: number;
}
