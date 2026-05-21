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
   */
  readonly skip_reason?: string;
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
   * `multi_document_yaml`, `line_out_of_range`, `monorepo_subpackage`,
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
}
