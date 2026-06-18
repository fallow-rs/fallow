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
  finding_id: "security:src/app.tsx:12",
  kind: "tainted-sink",
  path: "src/app.tsx",
  line: 12,
  col: 4,
  evidence: "reaches process.env.SECRET",
  severity: overrides.severity ?? "low",
  trace: [],
  actions: [],
  candidate: {
    sink: {
      path: overrides.path ?? "src/app.tsx",
      line: overrides.line ?? 12,
      col: overrides.col ?? 4,
      category: overrides.category,
      cwe: overrides.cwe,
    },
    boundary: {
      client_server: false,
      cross_module: false,
    },
  },
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

  it("forwards --workspace only when a workspace scope is set (#906 C2)", () => {
    expect(buildSecurityArgs({ configPath: "", changedSince: "" })).not.toContain("--workspace");
    expect(
      buildSecurityArgs({ configPath: "", changedSince: "", workspace: "" }),
    ).not.toContain("--workspace");
    const scoped = buildSecurityArgs({ configPath: "", changedSince: "", workspace: "pkg-a" });
    expect(scoped).toContain("--workspace");
    expect(scoped[scoped.indexOf("--workspace") + 1]).toBe("pkg-a");
  });
});

describe("countSecurityFindings", () => {
  it("returns 0 for null", () => {
    expect(countSecurityFindings(null)).toBe(0);
  });

  it("counts the findings array", () => {
    const result: SecurityOutput = {
      schema_version: "3",
      version: "test",
      elapsed_ms: 0,
      config: {
        rules: {
          security_client_server_leak: { configured: "off", effective: "warn" },
          security_sink: { configured: "off", effective: "warn" },
        },
        categories_include: null,
        categories_exclude: null,
      },
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
      ["untrusted-source", "untrusted source"],
      ["module-source", "source module"],
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

  it("supports explicit subcommand names without prefix matches", () => {
    expect(parseUnknownSubcommand('error: unrecognized subcommand "inspect"', "inspect")).toBe(
      true,
    );
    expect(parseUnknownSubcommand("error: unrecognized subcommand inspection", "inspect")).toBe(
      false,
    );
  });
});
