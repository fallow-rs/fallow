/**
 * Pure helpers for the license UI: parsing the CLI JSON, mapping a license
 * state to status-bar presentation, and validating activation input. No
 * `vscode` import so vitest runs these headless (same split as
 * `statusBar-utils.ts` vs `statusBar.ts`).
 */

import type {
  LicenseParseResult,
  LicenseState,
  LicenseStatusJson,
} from "./license-types.js";

/**
 * Theme color key the status-bar item uses for each severity. `null` =
 * neutral (no background). Mirrors `statusBar-utils.ts`'s `SeverityKey`.
 */
export type LicenseSeverityKey =
  | "statusBarItem.errorBackground"
  | "statusBarItem.warningBackground"
  | null;

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

/** Short, human-facing label for a license state (status-bar text suffix). */
export const licenseStateLabel = (state: LicenseState): string => {
  switch (state) {
    case "valid":
      return "active";
    case "expired_warning":
    case "expired_watermark":
    case "hard_fail":
      return "expired";
    case "missing":
      return "no license";
    default: {
      // Exhaustiveness guard: adding a state to the union without a label
      // here is a compile error, and the vitest test asserts a label exists
      // for every member.
      const never: never = state;
      return never;
    }
  }
};

/** Presentation parts for the license status-bar item. */
export interface LicenseStatusBarParts {
  /** Status-bar text, including a leading codicon. */
  readonly text: string;
  /** Trusted-markdown tooltip body. */
  readonly tooltipMd: string;
  /** Theme color key for the background, or `null` for neutral. */
  readonly severity: LicenseSeverityKey;
}

/**
 * Escape text destined for a trusted `MarkdownString`. Tier / feature strings
 * come from a verified JWT, not arbitrary input, but we escape defensively per
 * the global trusted-markdown rule (command-link injection vector). Strips the
 * markdown control characters that could break out of the tooltip.
 */
export const escapeMarkdown = (raw: string): string =>
  raw.replace(/[\\`*_{}[\]()#+\-.!|<>]/g, (ch) => `\\${ch}`);

const PLACEHOLDER_TEXT = "$(key) Fallow License";

/** Status-bar parts before any probe has run (or when probing is disabled). */
export const licensePlaceholderParts = (): LicenseStatusBarParts => ({
  text: PLACEHOLDER_TEXT,
  tooltipMd: "Fallow license status not checked yet.\n\nRun **Fallow: Show License Status**.",
  severity: null,
});

const expiryTooltipLine = (status: LicenseStatusJson): string => {
  if (status.days_until_expiry !== null) {
    const d = status.days_until_expiry;
    return `Expires in ${d} day${d === 1 ? "" : "s"}.`;
  }
  if (status.days_since_expiry !== null) {
    const d = status.days_since_expiry;
    return `Expired ${d} day${d === 1 ? "" : "s"} ago.`;
  }
  return "";
};

const claimsTooltipLines = (status: LicenseStatusJson): string[] => {
  const lines: string[] = [];
  if (status.tier !== null) {
    lines.push(`Tier: ${escapeMarkdown(status.tier)}`);
  }
  if (status.seats !== null) {
    lines.push(`Seats: ${status.seats}`);
  }
  if (status.features.length > 0) {
    lines.push(`Features: ${status.features.map(escapeMarkdown).join(", ")}`);
  }
  const expiry = expiryTooltipLine(status);
  if (expiry.length > 0) {
    lines.push(expiry);
  }
  return lines;
};

/**
 * Map a parsed license status to status-bar presentation. Tooltip states facts
 * only and never implies the license verifies findings (#903 adjacency): the
 * license is a cryptographic fact, distinct from heuristic security
 * candidates.
 */
export const licenseStatusBarParts = (status: LicenseStatusJson): LicenseStatusBarParts => {
  switch (status.state) {
    case "valid": {
      const tierLabel = status.tier !== null ? `: ${escapeMarkdown(status.tier)}` : "";
      const lines = claimsTooltipLines(status);
      if (status.refresh_suggested) {
        lines.push("Refresh recommended to stay ahead of expiry.");
      }
      return {
        text: `$(verified) Fallow${tierLabel}`,
        tooltipMd: ["**Fallow license active**", ...lines].join("\n\n"),
        severity: null,
      };
    }
    case "expired_warning":
    case "expired_watermark": {
      const lines = claimsTooltipLines(status);
      lines.push("Run **Fallow: Refresh License** to renew.");
      return {
        text: "$(warning) Fallow: expired",
        tooltipMd: ["**Fallow license expired**", ...lines].join("\n\n"),
        severity: "statusBarItem.warningBackground",
      };
    }
    case "hard_fail": {
      const lines = claimsTooltipLines(status);
      lines.push("Paid features are blocked. Run **Fallow: Refresh License**.");
      return {
        text: "$(error) Fallow: expired",
        tooltipMd: ["**Fallow license expired (past grace window)**", ...lines].join("\n\n"),
        severity: "statusBarItem.errorBackground",
      };
    }
    case "missing":
      return {
        text: "$(key) Fallow: no license",
        tooltipMd: [
          "**No Fallow license active**",
          "Start a 30-day trial or activate a license token.",
          "[Activate license](command:fallow.license.activate)",
        ].join("\n\n"),
        severity: null,
      };
    default: {
      const never: never = status.state;
      return never;
    }
  }
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
