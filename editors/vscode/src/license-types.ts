/**
 * Types for `fallow license <sub> --format json` output. The license JSON
 * shape is not part of `docs/output-schema.json` (it is not a finding /
 * duplication type), so these stay hand-written, mirroring `fix-types.ts`.
 *
 * Source of truth is `LicenseStatusJson` in
 * `crates/cli/src/license/mod.rs`. Keep `LicenseState` in lockstep with the
 * Rust `status_state` match arms; the vitest exhaustiveness guard in
 * `license-utils` fails when a state is added here without a label.
 */

/**
 * Machine discriminant for the license state. Mirrors the Rust grace ladder:
 * `valid` (active), `expired_warning` (analysis still runs, refresh nudge),
 * `expired_watermark` (output watermarked), `hard_fail` (paid features
 * blocked), `missing` (no license material found).
 */
export type LicenseState =
  | "valid"
  | "expired_warning"
  | "expired_watermark"
  | "hard_fail"
  | "missing";

/**
 * The `kind` discriminator the CLI stamps on each license JSON envelope.
 * Lets a consumer tell a status probe apart from a post-action result.
 */
export type LicenseKind =
  | "license-status"
  | "license-activate"
  | "license-refresh"
  | "license-deactivate";

/**
 * Parsed `fallow license <sub> --format json` success envelope. `tier`,
 * `seats`, and the expiry fields are `null` on the states that carry no
 * verified claims (`hard_fail` keeps claims but blocks features; `missing`
 * has none). `message` is the single human-facing sentence the UI shows
 * verbatim, so the extension never re-derives wording.
 */
export interface LicenseStatusJson {
  readonly kind: LicenseKind;
  readonly schema_version: number;
  readonly state: LicenseState;
  readonly tier: string | null;
  readonly seats: number | null;
  readonly features: ReadonlyArray<string>;
  readonly days_until_expiry: number | null;
  readonly days_since_expiry: number | null;
  readonly refresh_suggested: boolean;
  readonly runtime_coverage_enabled: boolean;
  readonly license_path: string;
  readonly message: string;
  /**
   * Present only on the `license-deactivate` envelope: whether a file was
   * actually removed (`false` when there was nothing to remove).
   */
  readonly removed?: boolean;
}

/**
 * Structured JSON error envelope the CLI emits on stdout under `--format
 * json` (shared shape across fallow commands).
 */
export interface LicenseErrorJson {
  readonly error: true;
  readonly message: string;
  readonly exit_code: number;
}

/**
 * Result of parsing license CLI stdout. `Result`-style discriminated union so
 * callers branch on `ok` rather than catching thrown parse errors.
 */
export type LicenseParseResult =
  | { readonly ok: true; readonly data: LicenseStatusJson }
  | { readonly ok: false; readonly error: string };

/**
 * Outcome of a license CLI invocation surfaced to the command handlers.
 * `status` is `null` when the action produced no parseable status (e.g. a
 * transport failure before the CLI wrote JSON).
 */
export interface LicenseActionResult {
  readonly ok: boolean;
  readonly status: LicenseStatusJson | null;
  readonly message: string;
}
