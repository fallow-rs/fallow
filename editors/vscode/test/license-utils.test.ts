import { describe, expect, it } from "vitest";
import {
  escapeMarkdown,
  licensePlaceholderParts,
  licenseStateLabel,
  licenseStatusBarParts,
  parseLicenseJson,
  validateEmail,
  validateJwtShape,
} from "../src/license-utils.js";
import type { LicenseState, LicenseStatusJson } from "../src/license-types.js";

const ALL_STATES: ReadonlyArray<LicenseState> = [
  "valid",
  "expired_warning",
  "expired_watermark",
  "hard_fail",
  "missing",
];

const status = (overrides: Partial<LicenseStatusJson> = {}): LicenseStatusJson => ({
  kind: "license-status",
  schema_version: 1,
  state: "valid",
  tier: "team",
  seats: 5,
  features: ["runtime_coverage"],
  days_until_expiry: 12,
  days_since_expiry: null,
  refresh_suggested: false,
  runtime_coverage_enabled: true,
  license_path: "/home/x/.fallow/license.jwt",
  message: "License active (team, 5 seats), 12 days until expiry.",
  ...overrides,
});

describe("parseLicenseJson", () => {
  it("parses a valid status envelope", () => {
    const result = parseLicenseJson(JSON.stringify(status()));
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.data.state).toBe("valid");
      expect(result.data.tier).toBe("team");
    }
  });

  it("parses a missing-state envelope with null claims", () => {
    const result = parseLicenseJson(
      JSON.stringify(
        status({ state: "missing", tier: null, seats: null, features: [], days_until_expiry: null }),
      ),
    );
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.data.state).toBe("missing");
      expect(result.data.tier).toBeNull();
    }
  });

  it("parses a hard_fail envelope", () => {
    const result = parseLicenseJson(
      JSON.stringify(status({ state: "hard_fail", runtime_coverage_enabled: false })),
    );
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.data.state).toBe("hard_fail");
    }
  });

  it("surfaces the structured error envelope message", () => {
    const result = parseLicenseJson(
      JSON.stringify({ error: true, message: "no license found", exit_code: 7 }),
    );
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.error).toBe("no license found");
    }
  });

  it("rejects an unknown state", () => {
    const result = parseLicenseJson(JSON.stringify({ ...status(), state: "frobnicated" }));
    expect(result.ok).toBe(false);
  });

  it("rejects malformed JSON", () => {
    const result = parseLicenseJson("not json {");
    expect(result.ok).toBe(false);
  });

  it("rejects empty output", () => {
    expect(parseLicenseJson("   ").ok).toBe(false);
  });
});

describe("licenseStatusBarParts", () => {
  it("renders valid with a verified icon and neutral background", () => {
    const parts = licenseStatusBarParts(status());
    expect(parts.text).toBe("$(verified) Fallow: team");
    expect(parts.severity).toBeNull();
    expect(parts.tooltipMd).toContain("Tier: team");
    expect(parts.tooltipMd).toContain("Expires in 12 days");
  });

  it("adds a refresh-recommended line when refresh_suggested is set", () => {
    const parts = licenseStatusBarParts(status({ refresh_suggested: true }));
    expect(parts.tooltipMd).toContain("Refresh recommended");
  });

  it("uses a warning background for expired_warning and expired_watermark", () => {
    for (const state of ["expired_warning", "expired_watermark"] as const) {
      const parts = licenseStatusBarParts(
        status({ state, days_until_expiry: null, days_since_expiry: 3 }),
      );
      expect(parts.text).toBe("$(warning) Fallow: expired");
      expect(parts.severity).toBe("statusBarItem.warningBackground");
      expect(parts.tooltipMd).toContain("Expired 3 days ago");
    }
  });

  it("uses an error background for hard_fail", () => {
    const parts = licenseStatusBarParts(
      status({ state: "hard_fail", days_until_expiry: null, days_since_expiry: 45 }),
    );
    expect(parts.text).toBe("$(error) Fallow: expired");
    expect(parts.severity).toBe("statusBarItem.errorBackground");
  });

  it("produces an activation call-to-action for missing", () => {
    const parts = licenseStatusBarParts(
      status({ state: "missing", tier: null, seats: null, features: [] }),
    );
    expect(parts.text).toBe("$(key) Fallow: no license");
    expect(parts.severity).toBeNull();
    expect(parts.tooltipMd).toContain("command:fallow.license.activate");
  });
});

describe("licensePlaceholderParts", () => {
  it("is neutral with the key icon", () => {
    const parts = licensePlaceholderParts();
    expect(parts.text).toBe("$(key) Fallow License");
    expect(parts.severity).toBeNull();
  });
});

describe("licenseStateLabel", () => {
  it("has a label for every state in the union (exhaustiveness guard)", () => {
    for (const state of ALL_STATES) {
      expect(licenseStateLabel(state).length).toBeGreaterThan(0);
    }
  });
});

describe("escapeMarkdown", () => {
  it("escapes characters that could break a trusted command link", () => {
    expect(escapeMarkdown("a]b)c")).toBe("a\\]b\\)c");
    expect(escapeMarkdown("plain")).toBe("plain");
  });
});

describe("validateJwtShape", () => {
  const longSegment = "a".repeat(20);
  const validJwt = `${longSegment}.${longSegment}.${longSegment}`;

  it("accepts a three-segment base64url token above the length floor", () => {
    expect(validateJwtShape(validJwt)).toBeNull();
  });

  it("tolerates folded whitespace like the CLI normalize_jwt", () => {
    expect(validateJwtShape(`${longSegment}.\n${longSegment}.  ${longSegment}`)).toBeNull();
  });

  it("rejects a two-segment token", () => {
    expect(validateJwtShape(`${longSegment}.${longSegment}`)).not.toBeNull();
  });

  it("rejects empty and whitespace-only input", () => {
    expect(validateJwtShape("")).not.toBeNull();
    expect(validateJwtShape("   ")).not.toBeNull();
  });

  it("rejects an obviously-truncated token", () => {
    expect(validateJwtShape("a.b.c")).not.toBeNull();
  });

  it("rejects non-base64url segment characters", () => {
    expect(validateJwtShape(`${longSegment}.${longSegment}.has spaces!!`)).not.toBeNull();
  });
});

describe("validateEmail", () => {
  it("accepts a normal address", () => {
    expect(validateEmail("a@b.co")).toBeNull();
  });

  it("rejects a missing @", () => {
    expect(validateEmail("abc.co")).not.toBeNull();
  });

  it("rejects empty input", () => {
    expect(validateEmail("")).not.toBeNull();
  });
});
