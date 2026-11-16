import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

interface FakeItem {
  id: string;
  text: string;
  name: string | undefined;
  command: string | undefined;
  tooltip:
    | { value: string; isTrusted: boolean | { enabledCommands: string[] }; supportThemeIcons: boolean }
    | undefined;
  accessibilityInformation: { label: string; role?: string } | undefined;
  backgroundColor: { id: string } | undefined;
  show: ReturnType<typeof vi.fn>;
  dispose: ReturnType<typeof vi.fn>;
}

const created: FakeItem[] = [];
const ONE_FOLDER = [{ name: "repo", uri: { fsPath: "/repo" } }];
const folders = vi.hoisted(() => ({
  workspaceFolders: [] as Array<{ name: string; uri: { fsPath: string } }>,
}));
const settings = { auditEnabled: true, healthStatusBar: true, licenseStatusBar: true };
const quickPick = vi.fn();
const executeCommand = vi.fn();

vi.mock("vscode", () => ({
  StatusBarAlignment: { Left: 1 },
  MarkdownString: class {
    public isTrusted = false;
    public supportThemeIcons = false;
    public constructor(public readonly value: string) {}
  },
  ThemeColor: class {
    public constructor(public readonly id: string) {}
  },
  workspace: folders,
  commands: { executeCommand: (...args: unknown[]) => executeCommand(...args) },
  window: {
    showQuickPick: (...args: unknown[]) => quickPick(...args),
    createStatusBarItem: (id: string) => {
      const item: FakeItem = {
        id,
        text: "",
        name: undefined,
        command: undefined,
        tooltip: undefined,
        accessibilityInformation: undefined,
        backgroundColor: undefined,
        show: vi.fn(),
        dispose: vi.fn(),
      };
      created.push(item);
      return item;
    },
  },
}));

vi.mock("../src/config.js", () => ({
  getChangedSince: () => "",
  getAuditEnabled: () => settings.auditEnabled,
  getHealthStatusBar: () => settings.healthStatusBar,
  getLicenseShowStatusBar: () => settings.licenseStatusBar,
}));

import {
  createStatusBar,
  disposeStatusBar,
  refreshStatusBar,
  setStatusBarAnalyzing,
  setStatusBarAudit,
  setStatusBarError,
  setStatusBarFindings,
  setStatusBarLicense,
  showStatusBarMenu,
  updateStatusBarFromLsp,
  updateStatusBarHealth,
} from "../src/statusBar.js";
import { HOVER_COMMANDS } from "../src/statusBar-model.js";
import { buildParamsFromCli } from "../src/statusBar-utils.js";
import type { AuditOutput, HealthOutput } from "../src/types.js";

const failAudit = { verdict: "fail", changed_files_count: 1, base_ref: "main", attribution: {
  gate: "new-only",
  dead_code_introduced: 1,
  complexity_introduced: 0,
  duplication_introduced: 0,
  styling_introduced: 0,
  diff_risk_introduced: 0,
}, summary: {} } as unknown as AuditOutput;

const health = { health_score: { score: 71.6, grade: "C" } } as unknown as HealthOutput;

beforeEach(() => {
  folders.workspaceFolders = ONE_FOLDER;
});

afterEach(() => {
  disposeStatusBar();
  created.length = 0;
  settings.auditEnabled = true;
  settings.healthStatusBar = true;
  settings.licenseStatusBar = true;
  quickPick.mockReset();
  executeCommand.mockReset();
});

describe("Fallow status bar item", () => {
  it("creates one item that opens the Fallow menu", () => {
    const item = createStatusBar() as unknown as FakeItem;
    expect(created).toHaveLength(1);
    expect(item.id).toBe("fallow.status");
    expect(item.name).toBe("Fallow");
    expect(item.command).toBe("fallow.showStatusMenu");
    expect(item.text).toBe("$(search) Fallow");
    expect(item.show).toHaveBeenCalled();
  });

  it("renders count and grade with a codicon hover and an accessibility label", () => {
    const item = createStatusBar() as unknown as FakeItem;
    updateStatusBarFromLsp({ ...buildParamsFromCli(null, null, null), totalIssues: 9 });
    updateStatusBarHealth(health);
    expect(item.text).toBe("$(search) Fallow 9 · C");
    expect(item.tooltip?.supportThemeIcons).toBe(true);
    expect(item.tooltip?.isTrusted).toEqual({ enabledCommands: [...HOVER_COMMANDS] });
    expect(item.tooltip?.value).toContain("Health: C \\(72\\)");
    expect(item.accessibilityInformation).toEqual({
      label: "Fallow: 9 findings, health grade C. Activate to open the Fallow menu.",
      role: "button",
    });
  });

  it("keeps the analyzing and error states when health updates", () => {
    const item = createStatusBar() as unknown as FakeItem;
    setStatusBarAnalyzing();
    updateStatusBarHealth(null);
    expect(item.text).toBe("$(loading~spin) Fallow: analyzing");
    setStatusBarError();
    updateStatusBarHealth(health);
    expect(item.text).toBe("$(error) Fallow: error");
  });

  it("turns the item red when the audit fails, and back when the setting is off", () => {
    const item = createStatusBar() as unknown as FakeItem;
    setStatusBarAudit({ kind: "result", audit: failAudit });
    expect(item.backgroundColor?.id).toBe("statusBarItem.errorBackground");
    settings.auditEnabled = false;
    refreshStatusBar();
    expect(item.backgroundColor).toBeUndefined();
  });

  it("hides the license state when fallow.license.showStatusBar is off", () => {
    const item = createStatusBar() as unknown as FakeItem;
    setStatusBarLicense({ kind: "unchecked" });
    expect(item.tooltip?.value).toContain("License");
    settings.licenseStatusBar = false;
    refreshStatusBar();
    expect(item.tooltip?.value).not.toContain("License");
  });

  it("runs the command of the picked menu row", async () => {
    createStatusBar();
    setStatusBarFindings({ mutedAll: true, mutedCategoryCount: 0 });
    quickPick.mockImplementation(async (rows: Array<{ label: string }>) =>
      rows.find((row) => row.label.includes("Show findings")),
    );
    await showStatusBarMenu();
    expect(executeCommand).toHaveBeenCalledWith("fallow.toggleAllDiagnostics");
  });

  it("runs nothing when the menu is dismissed", async () => {
    createStatusBar();
    quickPick.mockResolvedValue(undefined);
    await showStatusBarMenu();
    expect(executeCommand).not.toHaveBeenCalled();
  });

  it("names the analyzed folder in a multi-root window", () => {
    folders.workspaceFolders = [
      { name: "web", uri: { fsPath: "/web" } },
      { name: "api", uri: { fsPath: "/api" } },
    ];
    const item = createStatusBar() as unknown as FakeItem;
    expect(item.tooltip?.value).toContain("Folder: web (the first folder of this window)");
    folders.workspaceFolders = ONE_FOLDER;
    refreshStatusBar();
    expect(item.tooltip?.value).not.toContain("Folder:");
  });
});
