import { describe, expect, it } from "vitest";
import {
  AUDIT_CANDIDATE_HEADER,
  auditGatingSuffix,
  auditScopeSummary,
  auditVerdictPresentation,
  buildAuditArgs,
  buildAuditTooltipMarkdown,
  gatingCount,
  parseAuditOutput,
} from "../src/audit-utils.js";
import type { AuditOutput } from "../src/types.js";

const baseAuditArgsOptions = {
  production: false,
  changedSince: "",
  workspace: "",
  configPath: "",
  gate: "new-only" as const,
};

/**
 * Trimmed from a real `fallow audit --format json` pass envelope (no changed
 * files, default gate). Volatile fields (version, elapsed_ms, head_sha) are
 * dropped; nothing under test reads them.
 */
const passAudit: AuditOutput = {
  schema_version: 7,
  version: "2.88.3",
  command: "audit",
  verdict: "pass",
  changed_files_count: 0,
  base_ref: "main",
  elapsed_ms: 56,
  summary: {
    dead_code_issues: 0,
    dead_code_has_errors: false,
    complexity_findings: 0,
    max_cyclomatic: null,
    duplication_clone_groups: 0,
  },
  attribution: {
    gate: "new-only",
    dead_code_introduced: 0,
    dead_code_inherited: 0,
    complexity_introduced: 0,
    complexity_inherited: 0,
    duplication_introduced: 0,
    duplication_inherited: 0,
    styling_introduced: 0,
    styling_inherited: 0,
  },
};

/**
 * Trimmed from a real `fallow audit --format json --gate all` fail envelope
 * (one changed file with a dead-code finding, exit code 1).
 */
const failAudit: AuditOutput = {
  schema_version: 7,
  version: "2.88.3",
  command: "audit",
  verdict: "fail",
  changed_files_count: 1,
  base_ref: "main",
  elapsed_ms: 94,
  summary: {
    dead_code_issues: 1,
    dead_code_has_errors: true,
    complexity_findings: 0,
    max_cyclomatic: null,
    duplication_clone_groups: 0,
  },
  attribution: {
    gate: "all",
    dead_code_introduced: 0,
    dead_code_inherited: 0,
    complexity_introduced: 0,
    complexity_inherited: 0,
    duplication_introduced: 0,
    duplication_inherited: 0,
    styling_introduced: 0,
    styling_inherited: 0,
  },
};

const stylingHealth = (
  effectiveSeverity: "warn" | "error",
  introduced?: boolean,
): NonNullable<AuditOutput["complexity"]> => ({
  findings: [],
  summary: {
    files_analyzed: 1,
    functions_analyzed: 0,
    functions_above_threshold: 0,
    max_cyclomatic_threshold: 20,
    max_cognitive_threshold: 15,
    max_crap_threshold: 30,
    max_unit_size_threshold: 60,
    severity_critical_count: 0,
    severity_high_count: 0,
    severity_moderate_count: 0,
  },
  styling_findings: [
    {
      code: "css-selector-complexity",
      sub_kind: "high-specificity",
      path: "src/styles.css",
      line: 1,
      value: "#app .card .title",
      effective_severity: effectiveSeverity,
      introduced,
      actions: [],
    },
  ],
});

describe("buildAuditArgs", () => {
  it("emits audit as the first positional with the json/quiet flags by default", () => {
    expect(buildAuditArgs(baseAuditArgsOptions)).toEqual(["audit", "--format", "json", "--quiet"]);
  });

  it("does not append a gate flag for the default new-only gate", () => {
    expect(buildAuditArgs(baseAuditArgsOptions)).not.toContain("--gate");
  });

  it("appends --gate all when the gate is all", () => {
    const args = buildAuditArgs({ ...baseAuditArgsOptions, gate: "all" });
    expect(args).toEqual(["audit", "--format", "json", "--quiet", "--gate", "all"]);
  });

  it("forwards changedSince, workspace, production, and configPath with correct flag spelling", () => {
    const args = buildAuditArgs({
      production: true,
      changedSince: "main",
      workspace: "pkg-a",
      configPath: "/abs/.fallowrc.json",
      gate: "all",
    });
    expect(args).toEqual([
      "audit",
      "--format",
      "json",
      "--quiet",
      "--changed-since",
      "main",
      "--workspace",
      "pkg-a",
      "--production",
      "--config",
      "/abs/.fallowrc.json",
      "--gate",
      "all",
    ]);
  });

  it("forwards --workspace when a workspace scope is set", () => {
    const args = buildAuditArgs({ ...baseAuditArgsOptions, workspace: "@scope/pkg" });
    expect(args).toContain("--workspace");
    expect(args[args.indexOf("--workspace") + 1]).toBe("@scope/pkg");
  });

  it("omits optional flags when their values are empty strings", () => {
    const args = buildAuditArgs({
      production: false,
      changedSince: "",
      workspace: "",
      configPath: "",
      gate: "new-only",
    });
    expect(args).not.toContain("--changed-since");
    expect(args).not.toContain("--workspace");
    expect(args).not.toContain("--config");
    expect(args).not.toContain("--production");
  });
});

describe("auditVerdictPresentation", () => {
  it("maps pass to a pass icon with no background tint", () => {
    expect(auditVerdictPresentation("pass")).toEqual({
      icon: "$(pass)",
      label: "pass",
      background: null,
    });
  });

  it("maps warn to a warning icon and warning background", () => {
    expect(auditVerdictPresentation("warn")).toEqual({
      icon: "$(warning)",
      label: "warn",
      background: "statusBarItem.warningBackground",
    });
  });

  it("maps fail to an error icon and error background", () => {
    expect(auditVerdictPresentation("fail")).toEqual({
      icon: "$(error)",
      label: "fail",
      background: "statusBarItem.errorBackground",
    });
  });
});

describe("gatingCount", () => {
  it("sums the non-styling introduced attribution fields under the new-only gate", () => {
    const audit: AuditOutput = {
      ...passAudit,
      attribution: {
        gate: "new-only",
        dead_code_introduced: 2,
        dead_code_inherited: 9,
        complexity_introduced: 1,
        complexity_inherited: 4,
        duplication_introduced: 3,
        duplication_inherited: 7,
        styling_introduced: 2,
        styling_inherited: 5,
      },
    };
    expect(gatingCount(audit)).toBe(6);
  });

  it("sums the summary totals under the all gate", () => {
    const audit: AuditOutput = {
      ...failAudit,
      summary: {
        dead_code_issues: 4,
        dead_code_has_errors: true,
        complexity_findings: 2,
        max_cyclomatic: 30,
        duplication_clone_groups: 1,
      },
    };
    expect(gatingCount(audit)).toBe(7);
  });

  it("ignores inherited findings under new-only (a pass with inherited noise shows 0)", () => {
    const audit: AuditOutput = {
      ...passAudit,
      attribution: {
        gate: "new-only",
        dead_code_introduced: 0,
        dead_code_inherited: 5,
        complexity_introduced: 0,
        complexity_inherited: 2,
        duplication_introduced: 0,
        duplication_inherited: 1,
        styling_introduced: 0,
        styling_inherited: 3,
      },
    };
    expect(gatingCount(audit)).toBe(0);
  });

  it("does not gate an introduced warn-severity styling finding", () => {
    const audit: AuditOutput = {
      ...passAudit,
      verdict: "warn",
      complexity: stylingHealth("warn", true),
      attribution: {
        ...passAudit.attribution,
        styling_introduced: 1,
      },
    };
    expect(gatingCount(audit)).toBe(0);
  });

  it("gates an introduced error-severity styling finding under new-only", () => {
    const audit: AuditOutput = {
      ...failAudit,
      complexity: stylingHealth("error", true),
      attribution: {
        ...passAudit.attribution,
        styling_introduced: 1,
      },
    };
    expect(gatingCount(audit)).toBe(1);
  });

  it("gates an error-severity styling finding under all", () => {
    const audit: AuditOutput = {
      ...failAudit,
      summary: {
        ...failAudit.summary,
        dead_code_issues: 0,
        dead_code_has_errors: false,
      },
      complexity: stylingHealth("error"),
    };
    expect(gatingCount(audit)).toBe(1);
  });
});

describe("auditGatingSuffix", () => {
  it("is empty when nothing is gating", () => {
    expect(auditGatingSuffix(passAudit)).toBe("");
  });

  it("shows the count for a warn verdict with gating findings (not just fail)", () => {
    // Regression for #908 C2: a warn verdict previously suppressed the count,
    // so a warn read as a clean pass at a glance. The suffix is verdict-agnostic.
    const warnAudit: AuditOutput = {
      ...passAudit,
      verdict: "warn",
      attribution: {
        gate: "new-only",
        dead_code_introduced: 2,
        dead_code_inherited: 0,
        complexity_introduced: 1,
        complexity_inherited: 0,
        duplication_introduced: 0,
        duplication_inherited: 0,
        styling_introduced: 0,
        styling_inherited: 0,
      },
    };
    expect(auditGatingSuffix(warnAudit)).toBe(" (3)");
  });

  it("shows the count for a fail verdict", () => {
    const audit: AuditOutput = {
      ...failAudit,
      summary: {
        dead_code_issues: 4,
        dead_code_has_errors: true,
        complexity_findings: 0,
        max_cyclomatic: null,
        duplication_clone_groups: 0,
      },
    };
    expect(auditGatingSuffix(audit)).toBe(" (4)");
  });
});

describe("auditScopeSummary", () => {
  it("singularizes a single changed file", () => {
    expect(auditScopeSummary({ ...failAudit, changed_files_count: 1, base_ref: "main" })).toBe(
      "1 changed file vs main",
    );
  });

  it("pluralizes multiple changed files and includes the base ref", () => {
    expect(auditScopeSummary({ ...failAudit, changed_files_count: 3, base_ref: "develop" })).toBe(
      "3 changed files vs develop",
    );
  });

  it("abbreviates a merge-base SHA and appends its provenance", () => {
    expect(
      auditScopeSummary({
        ...failAudit,
        changed_files_count: 1,
        base_ref: "611d151e8250146426ff3178e94207f8a8d3cc7b",
        base_description: "merge-base with origin/main",
      }),
    ).toBe("1 changed file vs 611d151e8250 (merge-base with origin/main)");
  });

  it("leaves a non-SHA base ref untouched when provenance is present", () => {
    expect(
      auditScopeSummary({
        ...failAudit,
        changed_files_count: 2,
        base_ref: "origin/main",
        base_description: "FALLOW_AUDIT_BASE=origin/main",
      }),
    ).toBe("2 changed files vs origin/main (FALLOW_AUDIT_BASE=origin/main)");
  });
});

describe("buildAuditTooltipMarkdown", () => {
  it("includes the scope line with changed file count and base ref", () => {
    const md = buildAuditTooltipMarkdown(failAudit);
    expect(md).toContain("1 changed file vs main");
  });

  it("abbreviates a merge-base SHA and shows provenance in the scope line", () => {
    const md = buildAuditTooltipMarkdown({
      ...failAudit,
      changed_files_count: 1,
      base_ref: "611d151e8250146426ff3178e94207f8a8d3cc7b",
      base_description: "merge-base with origin/main",
    });
    // The tooltip is trusted markdown, so the hyphen in "merge-base" is escaped.
    expect(md).toContain("1 changed file vs 611d151e8250 (merge\\-base with origin/main)");
    expect(md).not.toContain("611d151e8250146426ff3178e94207f8a8d3cc7b");
  });

  it("lists only non-zero gating categories", () => {
    const audit: AuditOutput = {
      ...failAudit,
      summary: {
        dead_code_issues: 3,
        dead_code_has_errors: true,
        complexity_findings: 0,
        max_cyclomatic: null,
        duplication_clone_groups: 0,
      },
    };
    const md = buildAuditTooltipMarkdown(audit);
    expect(md).toContain("3 dead-code candidates");
    expect(md).not.toContain("complexity candidates");
    expect(md).not.toContain("duplication candidates");
    expect(md).not.toContain("styling candidates");
  });

  it("lists a styling-only gating category", () => {
    const audit: AuditOutput = {
      ...failAudit,
      summary: {
        ...failAudit.summary,
        dead_code_issues: 0,
        dead_code_has_errors: false,
      },
      complexity: stylingHealth("error"),
    };
    const md = buildAuditTooltipMarkdown(audit);
    expect(md).toContain("1 styling candidate");
  });

  it("includes both command links", () => {
    const md = buildAuditTooltipMarkdown(passAudit);
    expect(md).toContain("command:fallow.audit");
    expect(md).toContain("command:fallow.showOutput");
  });

  it("includes the candidate-framing header (never defect/vulnerability wording)", () => {
    const md = buildAuditTooltipMarkdown(passAudit);
    expect(md).toContain(AUDIT_CANDIDATE_HEADER);
    const lower = md.toLowerCase();
    expect(lower).not.toContain("vulnerability");
    expect(lower).not.toContain("vulnerabilities");
    expect(lower).not.toContain("defect");
  });

  it("escapes markdown metacharacters in the base ref", () => {
    const audit: AuditOutput = { ...passAudit, base_ref: "feature/x_(y)" };
    const md = buildAuditTooltipMarkdown(audit);
    expect(md).toContain("feature/x\\_\\(y\\)");
  });

  it("shows a no-candidates line when nothing is gating", () => {
    const md = buildAuditTooltipMarkdown(passAudit);
    expect(md).toContain("No gating candidates");
  });
});

describe("parseAuditOutput", () => {
  it("parses a valid audit envelope into an object", () => {
    const parsed = parseAuditOutput(JSON.stringify(passAudit));
    expect(parsed).not.toBeNull();
    expect(parsed?.verdict).toBe("pass");
  });

  it("returns null for empty or whitespace stdout", () => {
    expect(parseAuditOutput("")).toBeNull();
    expect(parseAuditOutput("   \n  ")).toBeNull();
  });

  it("returns null for a non-audit command or missing verdict", () => {
    expect(parseAuditOutput(JSON.stringify({ command: "check", verdict: "pass" }))).toBeNull();
    expect(parseAuditOutput(JSON.stringify({ command: "audit" }))).toBeNull();
  });

  it("returns null on malformed JSON", () => {
    expect(parseAuditOutput("{not json")).toBeNull();
  });

  it("parses a fail-verdict envelope (proves stdout is read even on exit 1)", () => {
    const parsed = parseAuditOutput(JSON.stringify(failAudit));
    expect(parsed?.verdict).toBe("fail");
  });
});
