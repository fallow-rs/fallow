import { describe, expect, it } from "vitest";
import {
  hasLicenseMaterial,
  parseLicenseJson,
  validateEmail,
  validateJwtShape,
} from "../src/license-utils.js";
import type { LicenseStatusJson } from "../src/license-types.js";

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

  it("parses a deactivate envelope carrying the full status shape", () => {
    // The parser trusts the full CLI status shape after it validates the state.
    const deactivate = {
      ...status({
        state: "missing",
        tier: null,
        seats: null,
        features: [],
        days_until_expiry: null,
        days_since_expiry: null,
        refresh_suggested: false,
        runtime_coverage_enabled: false,
      }),
      kind: "license-deactivate" as const,
      message: "License removed from /home/x/.fallow/license.jwt.",
      removed: true,
    };
    const result = parseLicenseJson(JSON.stringify(deactivate));
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.data.state).toBe("missing");
      expect(result.data.kind).toBe("license-deactivate");
      expect(result.data.removed).toBe(true);
      expect(result.data.runtime_coverage_enabled).toBe(false);
      expect(result.data.refresh_suggested).toBe(false);
      expect(result.data.features).toEqual([]);
      expect(result.data.days_since_expiry).toBeNull();
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

describe("hasLicenseMaterial", () => {
  const noFiles = (): boolean => false;
  const defaultPath = "/home/u/.fallow/license.jwt";

  it("is true when an inline $FALLOW_LICENSE JWT is set", () => {
    expect(hasLicenseMaterial("eyJ.payload.sig", undefined, defaultPath, noFiles)).toBe(true);
  });

  it("ignores a blank / whitespace-only inline value", () => {
    expect(hasLicenseMaterial("   ", undefined, defaultPath, noFiles)).toBe(false);
  });

  it("uses $FALLOW_LICENSE_PATH (and does NOT fall back to the default) when set", () => {
    const exists = (p: string): boolean => p === "/custom/license.jwt";
    expect(hasLicenseMaterial(undefined, "/custom/license.jwt", defaultPath, exists)).toBe(true);
    // Path env set but the file is missing => not present, default is never consulted.
    expect(hasLicenseMaterial(undefined, "/missing/license.jwt", defaultPath, exists)).toBe(false);
  });

  it("falls back to the default path when no env is set", () => {
    const exists = (p: string): boolean => p === defaultPath;
    expect(hasLicenseMaterial(undefined, undefined, defaultPath, exists)).toBe(true);
  });

  it("is false for a never-licensed machine (no env, no default file)", () => {
    expect(hasLicenseMaterial(undefined, undefined, defaultPath, noFiles)).toBe(false);
    expect(hasLicenseMaterial(undefined, "", defaultPath, noFiles)).toBe(false);
  });
});
