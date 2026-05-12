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
}

export interface FallowFixResult {
  readonly dry_run: boolean;
  readonly fixes: ReadonlyArray<FixAction>;
  readonly total_fixed: number;
}
