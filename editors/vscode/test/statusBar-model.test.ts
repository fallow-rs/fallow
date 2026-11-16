import { describe, expect, it } from "vitest";
import {
  HOVER_COMMANDS,
  LICENSE_EXPIRY_NOTICE_DAYS,
  buildStatusMenuEntries,
  initialStatusModel,
  licenseBarNotice,
  renderStatusAccessibilityLabel,
  renderStatusPresentation,
  renderStatusText,
  renderStatusTooltip,
  statusBackground,
} from "../src/statusBar-model.js";
import type { LicenseIndicator, StatusModel } from "../src/statusBar-model.js";
import { buildParamsFromCli } from "../src/statusBar-utils.js";
import type { AnalysisCompleteParams } from "../src/statusBar-utils.js";
import type { LicenseStatusJson } from "../src/license-types.js";
import type { AuditOutput } from "../src/types.js";
import { WORKSPACE_SCOPE_DISCLOSURE } from "../src/workspacePicker-utils.js";

const params = (overrides: Partial<AnalysisCompleteParams> = {}): AnalysisCompleteParams => ({
  ...buildParamsFromCli(null, null, null),
  ...overrides,
});

const audit = (verdict: AuditOutput["verdict"], introduced = 0): AuditOutput => ({
  schema_version: 14,
  version: "3.43.2",
  command: "audit",
  verdict,
  changed_files_count: 2,
  base_ref: "main",
  elapsed_ms: 10,
  summary: {
    dead_code_issues: introduced,
    dead_code_has_errors: false,
    complexity_findings: 0,
    max_cyclomatic: null,
    duplication_clone_groups: 0,
  },
  attribution: {
    gate: "new-only",
    dead_code_introduced: introduced,
    dead_code_inherited: 0,
    complexity_introduced: 0,
    complexity_inherited: 0,
    duplication_introduced: 0,
    duplication_inherited: 0,
    styling_introduced: 0,
    styling_inherited: 0,
    duplication_demoted: 0,
    diff_risk_introduced: 0,
  },
});

const license = (overrides: Partial<LicenseStatusJson> = {}): LicenseIndicator => ({
  kind: "status",
  status: {
    kind: "license-status",
    schema_version: 1,
    state: "valid",
    tier: "team",
    seats: 5,
    features: [],
    days_until_expiry: 200,
    days_since_expiry: null,
    refresh_suggested: false,
    runtime_coverage_enabled: true,
    license_path: "/home/x/.fallow/license.jwt",
    message: "License active.",
    ...overrides,
  },
});

/** The decision example: 9 findings, 3 clone groups, grade C. */
const resultModel = (overrides: Partial<StatusModel> = {}): StatusModel => ({
  ...initialStatusModel(),
  analysis: {
    kind: "result",
    params: params({ totalIssues: 9, unusedExports: 9, cloneGroups: 3, duplicationPercentage: 2.34 }),
  },
  health: { grade: "C", label: "C (72)" },
  ...overrides,
});

describe("renderStatusText", () => {
  it("shows the finding count and the health grade in one item", () => {
    expect(renderStatusText(resultModel())).toBe("$(search) Fallow 9 · C");
  });

  it("omits the grade when fallow.health.statusBar is off or health did not run", () => {
    expect(renderStatusText(resultModel({ showHealthGrade: false }))).toBe("$(search) Fallow 9");
    expect(renderStatusText(resultModel({ health: null }))).toBe("$(search) Fallow 9");
  });

  it("renders the idle, analyzing, error and no-folder states", () => {
    expect(renderStatusText(initialStatusModel())).toBe("$(search) Fallow");
    expect(renderStatusText({ ...initialStatusModel(), analysis: { kind: "analyzing" } })).toBe(
      "$(loading~spin) Fallow: analyzing",
    );
    expect(renderStatusText({ ...initialStatusModel(), analysis: { kind: "error" } })).toBe(
      "$(error) Fallow: error",
    );
    expect(renderStatusText({ ...resultModel(), hasWorkspaceFolder: false })).toBe(
      "$(search) Fallow: no folder",
    );
  });

  it("keeps the changed-since suffix in each state", () => {
    expect(renderStatusText(resultModel({ changedSince: "main" }))).toBe(
      "$(search) Fallow 9 · C (since main)",
    );
    expect(
      renderStatusText({ ...initialStatusModel(), analysis: { kind: "analyzing" }, changedSince: "main" }),
    ).toBe("$(loading~spin) Fallow: analyzing (since main)");
  });

  it("shows a spinner while the audit runs", () => {
    expect(renderStatusText(resultModel({ audit: { kind: "running" } }))).toBe(
      "$(loading~spin) Fallow 9 · C",
    );
  });

  it("marks a failed or warning audit with an icon, not with extra words", () => {
    expect(renderStatusText(resultModel({ audit: { kind: "result", audit: audit("fail", 2) } }))).toBe(
      "$(error) Fallow 9 · C",
    );
    expect(renderStatusText(resultModel({ audit: { kind: "result", audit: audit("warn") } }))).toBe(
      "$(warning) Fallow 9 · C",
    );
    expect(renderStatusText(resultModel({ audit: { kind: "result", audit: audit("pass") } }))).toBe(
      "$(search) Fallow 9 · C",
    );
  });

  it("ignores the audit when fallow.audit.statusBar.enabled is off", () => {
    const model = resultModel({ audit: { kind: "result", audit: audit("fail", 2) }, showAudit: false });
    expect(renderStatusText(model)).toBe("$(search) Fallow 9 · C");
    expect(statusBackground(model)).toBeNull();
  });

  it("shows a closed eye when all findings are hidden in the editor", () => {
    expect(
      renderStatusText(resultModel({ findings: { mutedAll: true, mutedCategoryCount: 0 } })),
    ).toBe("$(eye-closed) Fallow 9 · C");
  });

  it("shows the license only when it needs action", () => {
    expect(renderStatusText(resultModel({ license: license() }))).toBe("$(search) Fallow 9 · C");
    expect(renderStatusText(resultModel({ license: license({ days_until_expiry: 5 }) }))).toBe(
      "$(search) Fallow 9 · C · $(key) license: 5 days left",
    );
    expect(
      renderStatusText(
        resultModel({
          license: license({ state: "expired_warning", days_until_expiry: null, days_since_expiry: 2 }),
        }),
      ),
    ).toBe("$(search) Fallow 9 · C · $(warning) license expired");
    expect(renderStatusText(resultModel({ license: license({ state: "missing" }) }))).toBe(
      "$(search) Fallow 9 · C · $(warning) license invalid",
    );
  });
});

describe("licenseBarNotice", () => {
  it("starts at the notice threshold", () => {
    expect(licenseBarNotice(license({ days_until_expiry: LICENSE_EXPIRY_NOTICE_DAYS + 1 }))).toBeNull();
    expect(licenseBarNotice(license({ days_until_expiry: LICENSE_EXPIRY_NOTICE_DAYS }))).toBe(
      `$(key) license: ${LICENSE_EXPIRY_NOTICE_DAYS} days left`,
    );
    expect(licenseBarNotice(license({ days_until_expiry: 1 }))).toBe("$(key) license: 1 day left");
  });

  it("uses an error icon past the grace window", () => {
    expect(licenseBarNotice(license({ state: "hard_fail", days_until_expiry: null }))).toBe(
      "$(error) license expired",
    );
  });

  it("shows nothing for a hidden or unchecked license", () => {
    expect(licenseBarNotice({ kind: "hidden" })).toBeNull();
    expect(licenseBarNotice({ kind: "unchecked" })).toBeNull();
  });
});

describe("statusBackground", () => {
  it("uses the error background when the audit fails or its run fails", () => {
    expect(statusBackground(resultModel({ audit: { kind: "result", audit: audit("fail", 1) } }))).toBe(
      "statusBarItem.errorBackground",
    );
    expect(statusBackground(resultModel({ audit: { kind: "error" } }))).toBe(
      "statusBarItem.errorBackground",
    );
  });

  it("uses the warning background for a warn verdict and none otherwise", () => {
    expect(statusBackground(resultModel({ audit: { kind: "result", audit: audit("warn") } }))).toBe(
      "statusBarItem.warningBackground",
    );
    expect(statusBackground(resultModel())).toBeNull();
    expect(statusBackground({ ...initialStatusModel(), analysis: { kind: "error" } })).toBeNull();
  });
});

describe("renderStatusTooltip", () => {
  it("shows duplication, health, audit, license, workspace and visibility", () => {
    const markdown = renderStatusTooltip(
      resultModel({
        audit: { kind: "result", audit: audit("fail", 2) },
        license: license(),
        workspace: { pickerAvailable: true, active: "web" },
      }),
    );
    expect(markdown).toContain("**Fallow** (workspace: web)");
    expect(markdown).toContain("$(warning) 9 unused exports");
    expect(markdown).toContain("$(copy) Duplication: 2.3% (3 clone groups)");
    expect(markdown).toContain("$(pulse) Health: C \\(72\\)");
    expect(markdown).toContain("$(error) Audit: fail (2 gating candidates)");
    expect(markdown).toContain("$(git-branch) 2 changed files vs main");
    expect(markdown).toContain("$(verified) License: active (team), expires in 200 days.");
    expect(markdown).toContain("$(layers) Workspace: web.");
    expect(markdown).toContain(WORKSPACE_SCOPE_DISCLOSURE);
    expect(markdown).toContain("$(eye) Findings in the editor: visible.");
    expect(markdown).toContain("command:fallow.analyze");
  });

  it("renders each analysis and audit state", () => {
    const base = initialStatusModel();
    expect(renderStatusTooltip(base)).toContain("Analysis: not run yet.");
    expect(renderStatusTooltip(base)).toContain("Audit: not run yet. [Run audit](command:fallow.audit)");
    expect(renderStatusTooltip({ ...base, analysis: { kind: "analyzing" } })).toContain(
      "$(loading~spin) Analysis: running.",
    );
    expect(renderStatusTooltip({ ...base, analysis: { kind: "error" } })).toContain(
      "$(error) Analysis: the run failed.",
    );
    expect(renderStatusTooltip({ ...base, audit: { kind: "running" } })).toContain(
      "$(loading~spin) Audit: running.",
    );
    expect(renderStatusTooltip({ ...base, audit: { kind: "error" } })).toContain(
      "$(error) Audit: the run failed.",
    );
    expect(renderStatusTooltip({ ...base, showAudit: false })).not.toContain("Audit");
  });

  it("states hidden findings and partly hidden categories", () => {
    expect(
      renderStatusTooltip(resultModel({ findings: { mutedAll: true, mutedCategoryCount: 0 } })),
    ).toContain("$(eye-closed) Findings in the editor: hidden.");
    expect(
      renderStatusTooltip(resultModel({ findings: { mutedAll: false, mutedCategoryCount: 2 } })),
    ).toContain("visible (2 categories hidden)");
  });

  it("shows the whole-project scope only on a multi-package repo", () => {
    expect(
      renderStatusTooltip(resultModel({ workspace: { pickerAvailable: true, active: "" } })),
    ).toContain("$(layers) Workspace: whole project.");
    expect(renderStatusTooltip(resultModel())).not.toContain("Workspace:");
  });

  it("shows the license state in the hover, and nothing for a machine with no license", () => {
    expect(renderStatusTooltip(resultModel())).not.toContain("License");
    expect(renderStatusTooltip(resultModel({ license: { kind: "unchecked" } }))).toContain(
      "License: not checked yet",
    );
    expect(
      renderStatusTooltip(resultModel({ license: license({ state: "hard_fail", days_until_expiry: null }) })),
    ).toContain("paid features are blocked");
  });

  it("asks for a folder when no folder is open", () => {
    expect(renderStatusTooltip({ ...resultModel(), hasWorkspaceFolder: false })).toContain(
      "Open a folder",
    );
  });

  it("escapes a workspace name in the trusted hover", () => {
    const markdown = renderStatusTooltip(
      resultModel({ workspace: { pickerAvailable: true, active: "[x](command:evil)" } }),
    );
    expect(markdown).not.toContain("[x](command:evil)");
  });
});

describe("renderStatusAccessibilityLabel", () => {
  it("reads the state as plain text without codicon names", () => {
    const label = renderStatusAccessibilityLabel(
      resultModel({
        audit: { kind: "result", audit: audit("fail", 2) },
        license: license({ days_until_expiry: 3 }),
        findings: { mutedAll: true, mutedCategoryCount: 0 },
      }),
    );
    expect(label).toBe(
      "Fallow: 9 findings, health grade C, audit fail, license: 3 days left, findings hidden in the editor. Activate to open the Fallow menu.",
    );
    expect(label).not.toContain("$(");
  });

  it("covers the analyzing and no-folder states", () => {
    expect(
      renderStatusAccessibilityLabel({ ...initialStatusModel(), analysis: { kind: "analyzing" } }),
    ).toBe("Fallow: analysis running. Activate to open the Fallow menu.");
    expect(renderStatusAccessibilityLabel({ ...initialStatusModel(), hasWorkspaceFolder: false })).toBe(
      "Fallow: no folder is open.",
    );
  });
});

describe("renderStatusPresentation", () => {
  it("combines text, hover, label and background", () => {
    const presentation = renderStatusPresentation(
      resultModel({ audit: { kind: "result", audit: audit("fail", 1) } }),
    );
    expect(presentation.text).toBe("$(error) Fallow 9 · C");
    expect(presentation.background).toBe("statusBarItem.errorBackground");
    expect(presentation.tooltipMarkdown).toContain("Audit: fail");
    expect(presentation.accessibilityLabel).toContain("audit fail");
  });
});

describe("buildStatusMenuEntries", () => {
  const commands = (model: StatusModel): string[] =>
    buildStatusMenuEntries(model).map((entry) => entry.command);

  it("offers analysis, hide findings, audit and settings on a single-package repo", () => {
    expect(commands(resultModel())).toEqual([
      "fallow.analyze",
      "fallow.toggleAllDiagnostics",
      "fallow.audit",
      "fallow.openSettings",
    ]);
    expect(buildStatusMenuEntries(resultModel())[1]?.label).toBe(
      "$(eye-closed) Hide findings in the editor",
    );
  });

  it("offers show findings when all findings are hidden", () => {
    const entries = buildStatusMenuEntries(
      resultModel({ findings: { mutedAll: true, mutedCategoryCount: 0 } }),
    );
    expect(entries[1]).toEqual({
      label: "$(eye) Show findings in the editor",
      command: "fallow.toggleAllDiagnostics",
    });
  });

  it("adds the workspace switch on a multi-package repo", () => {
    const entries = buildStatusMenuEntries(
      resultModel({ workspace: { pickerAvailable: true, active: "web" } }),
    );
    expect(entries.find((entry) => entry.command === "fallow.selectWorkspace")).toEqual({
      label: "$(layers) Switch workspace",
      description: "web",
      command: "fallow.selectWorkspace",
    });
  });

  it("shows the last audit verdict next to Run audit", () => {
    const entries = buildStatusMenuEntries(
      resultModel({ audit: { kind: "result", audit: audit("fail", 1) } }),
    );
    expect(entries.find((entry) => entry.command === "fallow.audit")?.description).toBe(
      "last verdict: fail",
    );
  });

  it("adds a license action only when the license needs action", () => {
    expect(commands(resultModel({ license: license() }))).not.toContain("fallow.license.refresh");
    expect(commands(resultModel({ license: license({ days_until_expiry: 2 }) }))).toContain(
      "fallow.license.refresh",
    );
    expect(commands(resultModel({ license: license({ state: "missing" }) }))).toContain(
      "fallow.license.activate",
    );
  });
});

describe("HOVER_COMMANDS", () => {
  it("lists every command that a hover links to", () => {
    const models: StatusModel[] = [
      initialStatusModel(),
      resultModel({
        audit: { kind: "result", audit: audit("fail", 1) },
        license: license({ days_until_expiry: 2 }),
        workspace: { pickerAvailable: true, active: "web" },
      }),
      resultModel({ license: { kind: "unchecked" } }),
      resultModel({ license: license({ state: "missing" }) }),
      resultModel({ license: license({ state: "hard_fail", days_until_expiry: null }) }),
    ];
    const linked = new Set(
      models.flatMap((model) =>
        [...renderStatusTooltip(model).matchAll(/\(command:([\w.]+)\)/g)].map((m) => m[1]),
      ),
    );
    expect([...linked].sort()).toEqual([...HOVER_COMMANDS].sort());
  });
});

describe("hidden findings with other states", () => {
  const hidden = { mutedAll: true, mutedCategoryCount: 0 };

  it("keeps the closed eye next to each audit state", () => {
    const cases: Array<[StatusModel["audit"], string]> = [
      [{ kind: "idle" }, "$(eye-closed) Fallow 9 · C"],
      [{ kind: "running" }, "$(loading~spin) $(eye-closed) Fallow 9 · C"],
      [{ kind: "error" }, "$(error) $(eye-closed) Fallow 9 · C"],
      [{ kind: "result", audit: audit("pass") }, "$(eye-closed) Fallow 9 · C"],
      [{ kind: "result", audit: audit("warn") }, "$(warning) $(eye-closed) Fallow 9 · C"],
      [{ kind: "result", audit: audit("fail", 1) }, "$(error) $(eye-closed) Fallow 9 · C"],
    ];
    for (const [auditState, text] of cases) {
      expect(renderStatusText(resultModel({ audit: auditState, findings: hidden }))).toBe(text);
    }
  });

  it("keeps the closed eye while the analysis runs or fails", () => {
    expect(
      renderStatusText({ ...initialStatusModel(), analysis: { kind: "analyzing" }, findings: hidden }),
    ).toBe("$(loading~spin) $(eye-closed) Fallow: analyzing");
    expect(
      renderStatusText({ ...initialStatusModel(), analysis: { kind: "error" }, findings: hidden }),
    ).toBe("$(error) $(eye-closed) Fallow: error");
  });
});

describe("license refresh suggestion", () => {
  const suggested = license({ days_until_expiry: 200, refresh_suggested: true });

  it("offers a refresh in the hover and the menu, not in the bar text", () => {
    expect(renderStatusText(resultModel({ license: suggested }))).toBe("$(search) Fallow 9 · C");
    const markdown = renderStatusTooltip(resultModel({ license: suggested }));
    expect(markdown).toContain("Refresh recommended.");
    expect(markdown).toContain("[Refresh](command:fallow.license.refresh)");
    expect(
      buildStatusMenuEntries(resultModel({ license: suggested })).map((entry) => entry.command),
    ).toContain("fallow.license.refresh");
  });

  it("offers no refresh when the CLI does not suggest one", () => {
    const markdown = renderStatusTooltip(resultModel({ license: license() }));
    expect(markdown).not.toContain("command:fallow.license.refresh");
  });
});

describe("license backgrounds", () => {
  const hardFail = license({ state: "hard_fail", days_until_expiry: null, days_since_expiry: 40 });

  it("uses the warning background when paid features are blocked", () => {
    expect(statusBackground(resultModel({ license: hardFail }))).toBe(
      "statusBarItem.warningBackground",
    );
    expect(renderStatusText(resultModel({ license: hardFail }))).toBe(
      "$(search) Fallow 9 · C · $(error) license expired",
    );
  });

  it("lets a failed audit win over a blocked license", () => {
    expect(
      statusBackground(
        resultModel({ license: hardFail, audit: { kind: "result", audit: audit("fail", 1) } }),
      ),
    ).toBe("statusBarItem.errorBackground");
  });

  it("gives an expired license that still works a notice without a background", () => {
    for (const state of ["expired_warning", "expired_watermark"] as const) {
      const model = resultModel({
        license: license({ state, days_until_expiry: null, days_since_expiry: 3 }),
      });
      expect(statusBackground(model)).toBeNull();
      expect(renderStatusText(model)).toContain("license expired");
    }
  });

  it("ignores the license when it is hidden", () => {
    expect(statusBackground(resultModel({ license: { kind: "hidden" } }))).toBeNull();
  });
});

describe("license details in the hover", () => {
  it("lists seats and features of an active license", () => {
    const markdown = renderStatusTooltip(
      resultModel({ license: license({ features: ["runtime_coverage", "audit"] }) }),
    );
    expect(markdown).toContain("Seats: 5");
    expect(markdown).toContain("Features: runtime\\_coverage, audit");
  });

  it("states how long ago an expired license expired", () => {
    const markdown = renderStatusTooltip(
      resultModel({
        license: license({ state: "expired_warning", days_until_expiry: null, days_since_expiry: 3 }),
      }),
    );
    expect(markdown).toContain("Expired 3 days ago.");
    expect(
      renderStatusTooltip(
        resultModel({
          license: license({ state: "hard_fail", days_until_expiry: null, days_since_expiry: 1 }),
        }),
      ),
    ).toContain("Expired 1 day ago.");
  });
});

describe("multi-root windows", () => {
  it("names the folder that the count comes from", () => {
    const markdown = renderStatusTooltip(resultModel({ analysisFolder: "web-app" }));
    expect(markdown).toContain("$(root-folder) Folder: web\\-app (the first folder of this window)");
  });

  it("names no folder in a single-folder window", () => {
    expect(renderStatusTooltip(resultModel())).not.toContain("Folder:");
  });
});
