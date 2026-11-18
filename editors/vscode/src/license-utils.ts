/**
 * Pure helpers for the license UI: parsing the CLI JSON, checking for license
 * material, and validating activation input. `statusBar-model.ts` maps a
 * license state to the status bar presentation. No
 * `vscode` import so vitest runs these headless (same split as
 * `statusBar-utils.ts` vs `statusBar.ts`).
 */

import type { LicenseParseResult, LicenseState, LicenseStatusJson } from "./license-types.js";

const KNOWN_STATES: ReadonlySet<string> = new Set<LicenseState>([
  "valid",
  "expired_warning",
  "expired_watermark",
  "hard_fail",
  "missing",
]);

const isLicenseState = (value: unknown): value is LicenseState =>
  typeof value === "string" && KNOWN_STATES.has(value);

/**
 * Parse `fallow license <sub> --format json` stdout into a typed status.
 *
 * Handles the structured-error envelope (`{ error: true, message }`) by
 * surfacing its message as a parse failure, validates that `state` is a known
 * union member before trusting the payload, and never throws: malformed JSON
 * returns `{ ok: false }`.
 */
export const parseLicenseJson = (stdout: string): LicenseParseResult => {
  const trimmed = stdout.trim();
  if (trimmed.length === 0) {
    return { ok: false, error: "fallow license produced no output." };
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return { ok: false, error: "Could not parse fallow license output as JSON." };
  }

  if (typeof parsed !== "object" || parsed === null) {
    return { ok: false, error: "Unexpected fallow license output." };
  }

  const record = parsed as Record<string, unknown>;

  if (record.error === true) {
    const message =
      typeof record.message === "string" && record.message.length > 0
        ? record.message
        : "fallow license reported an error.";
    return { ok: false, error: message };
  }

  if (!isLicenseState(record.state)) {
    return { ok: false, error: "fallow license output is missing a known state." };
  }

  return { ok: true, data: record as unknown as LicenseStatusJson };
};

/**
 * Report whether a license source has material. Follow the same precedence as
 * `fallow_license::load_raw_jwt`: `$FALLOW_LICENSE`, `$FALLOW_LICENSE_PATH`,
 * then `~/.fallow/license.jwt`.
 */
export const hasLicenseMaterial = (
  inlineEnv: string | undefined,
  pathEnv: string | undefined,
  defaultPath: string,
  fileExists: (filePath: string) => boolean,
): boolean => {
  if (inlineEnv !== undefined && inlineEnv.trim().length > 0) {
    return true;
  }
  const explicitPath = pathEnv?.trim();
  if (explicitPath !== undefined && explicitPath.length > 0) {
    // `$FALLOW_LICENSE_PATH` takes over from the default entirely: the Rust
    // loader does not fall back to `~/.fallow/license.jwt` once it is set.
    return fileExists(explicitPath);
  }
  return fileExists(defaultPath);
};

const JWT_SEGMENT = /^[A-Za-z0-9_-]+$/;

/**
 * Validate the shape of a pasted JWT before it is sent to the CLI. Mirrors the
 * CLI `normalize_jwt` whitespace strip (the CLI tolerates folded tokens), then
 * requires three non-empty base64url segments above a length floor. Returns an
 * error message for `validateInput`, or `null` when the input is acceptable.
 */
export const validateJwtShape = (raw: string): string | null => {
  const stripped = raw.replace(/\s+/g, "");
  if (stripped.length === 0) {
    return "Paste a license token.";
  }
  const segments = stripped.split(".");
  if (segments.length !== 3 || segments.some((s) => s.length === 0)) {
    return "A license token has three dot-separated segments.";
  }
  if (segments.some((s) => !JWT_SEGMENT.test(s))) {
    return "License token segments must be base64url (A-Z, a-z, 0-9, -, _).";
  }
  // A signed EdDSA JWT with real claims is comfortably longer than this; the
  // floor only rejects obviously-truncated paste.
  if (stripped.length < 40) {
    return "That token looks too short to be a license.";
  }
  return null;
};

/** `true` when the pasted token passes {@link validateJwtShape}. */
export const isValidJwtShape = (raw: string): boolean => validateJwtShape(raw) === null;

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

/**
 * Validate a trial email. Returns an error message for `validateInput`, or
 * `null` when acceptable. Deliberately permissive (one `@`, one dot in the
 * domain); the backend is the real authority.
 */
export const validateEmail = (raw: string): string | null => {
  const trimmed = raw.trim();
  if (trimmed.length === 0) {
    return "Enter an email address for the trial.";
  }
  if (!EMAIL.test(trimmed)) {
    return "Enter a valid email address.";
  }
  return null;
};

/** `true` when the email passes {@link validateEmail}. */
export const isValidEmail = (raw: string): boolean => validateEmail(raw) === null;
