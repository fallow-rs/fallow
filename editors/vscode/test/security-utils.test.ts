import { describe, expect, it } from "vitest";
import {
  buildSecurityArgs,
  countSecurityFindings,
  hopRoleLabel,
  parseUnknownSubcommand,
  securityFindingLabel,
} from "../src/security-utils.js";
import type { SecurityFinding, SecurityOutput, TraceHopRole } from "../src/types.js";

const finding = (overrides: Partial<SecurityFinding>): SecurityFinding => ({
  kind: "tainted-sink",
  path: "src/app.tsx",
  line: 12,
  col: 4,
  evidence: "reaches process.env.SECRET",
  trace: [],
  actions: [],
  ...overrides,
});

describe("buildSecurityArgs", () => {
  it("emits the base security argv", () => {
    expect(buildSecurityArgs({ configPath: "", changedSince: "" })).toEqual([
      "security",
      "--format",
      "json",
      "--quiet",
    ]);
  });

  it("adds --changed-since and --config when set", () => {
    expect(
      buildSecurityArgs({ configPath: "/abs/.fallowrc.json", changedSince: "main" }),
    ).toEqual([
      "security",
      "--format",
      "json",
      "--quiet",
      "--changed-since",
      "main",
      "--config",
      "/abs/.fallowrc.json",
    ]);
  });

  it("never emits --production or any --dupes-* flag (rejected by `fallow security`)", () => {
    const args = buildSecurityArgs({ configPath: "/abs/cfg.json", changedSince: "HEAD~3" });
    expect(args).not.toContain("--production");
    expect(args.some((arg) => arg.startsWith("--dupes"))).toBe(false);
  });
});

describe("countSecurityFindings", () => {
  it("returns 0 for null", () => {
    expect(countSecurityFindings(null)).toBe(0);
  });

  it("counts the findings array", () => {
    const result: SecurityOutput = {
      schema_version: "1",
      security_findings: [finding({}), finding({})],
      unresolved_edge_files: 0,
      unresolved_callee_sites: 0,
    };
    expect(countSecurityFindings(result)).toBe(2);
  });
});

describe("securityFindingLabel", () => {
  it("labels a client-server-leak by its bespoke kind", () => {
    expect(securityFindingLabel(finding({ kind: "client-server-leak" }))).toBe(
      "client-server-leak",
    );
  });

  it("labels a tainted-sink with category and CWE", () => {
    expect(
      securityFindingLabel(finding({ kind: "tainted-sink", category: "dangerous-html", cwe: 79 })),
    ).toBe("dangerous-html (CWE-79)");
  });

  it("labels a tainted-sink with category only", () => {
    expect(
      securityFindingLabel(finding({ kind: "tainted-sink", category: "dangerous-html" })),
    ).toBe("dangerous-html");
  });

  it("falls back to tainted-sink when neither category nor cwe is present", () => {
    expect(securityFindingLabel(finding({ kind: "tainted-sink" }))).toBe("tainted-sink");
  });
});

describe("hopRoleLabel", () => {
  it("maps every TraceHopRole to its human label", () => {
    const cases: ReadonlyArray<readonly [TraceHopRole, string]> = [
      ["client-boundary", "client boundary"],
      ["intermediate", "intermediate"],
      ["secret-source", "secret source"],
      ["sink", "sink site"],
    ];
    for (const [role, label] of cases) {
      expect(hopRoleLabel(role)).toBe(label);
    }
  });
});

describe("parseUnknownSubcommand", () => {
  it("detects the modern clap unrecognized-subcommand error", () => {
    expect(parseUnknownSubcommand("error: unrecognized subcommand 'security'")).toBe(true);
  });

  it("detects the legacy clap phrasing", () => {
    expect(parseUnknownSubcommand("The subcommand 'security' wasn't recognized")).toBe(true);
  });

  it("returns false for unrelated errors", () => {
    expect(parseUnknownSubcommand("fallow exited with code 2")).toBe(false);
    expect(parseUnknownSubcommand("unrecognized subcommand 'health'")).toBe(false);
  });
});
