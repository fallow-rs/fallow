// VS Code calls TreeDataProvider members through the registered provider.
// fallow-ignore-file unused-class-member
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  countCoverageItems,
  formatConfidence,
  sortHotPaths,
  splitCleanupCandidates,
} from "./coverage-utils.js";
import { middleElidePath, resolveFilePath as resolveFilePathPure } from "./treeView-utils.js";
import type {
  RuntimeCoverageFinding,
  RuntimeCoverageHotPath,
  RuntimeCoverageReport,
} from "./types.js";

const resolveFilePath = (filePath: string | undefined) =>
  resolveFilePathPure(filePath, vscode.workspace.workspaceFolders?.[0]?.uri.fsPath);

/** Open-at-line command shared by every leaf, 1-indexed line in, 0-indexed out. */
const openAtLine = (absolute: string, line: number): vscode.Command => ({
  command: "vscode.open",
  title: "Open File",
  arguments: [
    vscode.Uri.file(absolute),
    { selection: new vscode.Range(Math.max(0, line - 1), 0, Math.max(0, line - 1), 0) },
  ],
});

class CoverageGroupItem extends vscode.TreeItem {
  constructor(
    label: string,
    icon: string,
    readonly children: ReadonlyArray<CoverageLeafItem>,
  ) {
    super(`${label} (${children.length})`, vscode.TreeItemCollapsibleState.Collapsed);
    this.contextValue = "coverageGroup";
    this.iconPath = new vscode.ThemeIcon(icon);
  }
}

class CoverageLeafItem extends vscode.TreeItem {
  constructor(label: string, filePath: string, line: number, icon: string, tooltip: string) {
    super(label, vscode.TreeItemCollapsibleState.None);

    const { absolute, relative } = resolveFilePath(filePath);

    this.description = `${middleElidePath(relative)}:${line}`;
    this.tooltip = tooltip;
    this.contextValue = "coverageItem";
    this.iconPath = new vscode.ThemeIcon(icon);
    this.command = openAtLine(absolute, line);
  }
}

const hotPathLeaf = (hot: RuntimeCoverageHotPath): CoverageLeafItem => {
  const { absolute } = resolveFilePath(hot.path);
  const tooltip = `${hot.function}\n${absolute}:${hot.line}\nInvocations: ${hot.invocations} (percentile ${hot.percentile})`;
  return new CoverageLeafItem(hot.function, hot.path, hot.line, "flame", tooltip);
};

const findingLeaf = (finding: RuntimeCoverageFinding, candidateNote: string): CoverageLeafItem => {
  const { absolute } = resolveFilePath(finding.path);
  const invocations = finding.invocations ?? 0;
  const tooltip = `${finding.function}\n${absolute}:${finding.line}\n${candidateNote}\nInvocations: ${invocations} · confidence: ${formatConfidence(finding.confidence)}`;
  const icon = finding.verdict === "safe_to_delete" ? "trash" : "eye";
  return new CoverageLeafItem(finding.function, finding.path, finding.line, icon, tooltip);
};

type CoverageItem = CoverageGroupItem | CoverageLeafItem;

/**
 * Tree provider for the Runtime Coverage view. Renders three lazily-expanding
 * groups from a single `coverage analyze` capture: hot paths (busiest first),
 * safe-to-delete candidates, and review-required candidates. All cleanup
 * findings are framed as CANDIDATES pending verification (#903); clicking one
 * only navigates to source, never deletes. Empty (null report) yields no
 * children so `viewsWelcome` renders the call-to-action.
 */
export class RuntimeCoverageTreeProvider implements vscode.TreeDataProvider<CoverageItem> {
  private report: RuntimeCoverageReport | null = null;
  private view: vscode.TreeView<CoverageItem> | null = null;

  private readonly _onDidChangeTreeData = new vscode.EventEmitter<
    CoverageItem | undefined | null | void
  >();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  setView(view: vscode.TreeView<CoverageItem>): void {
    this.view = view;
  }

  update(report: RuntimeCoverageReport | null): void {
    this.report = report;
    this._onDidChangeTreeData.fire();
    this.updateBadge();
  }

  private updateBadge(): void {
    if (!this.view) {
      return;
    }
    const count = countCoverageItems(this.report);
    this.view.badge =
      count > 0
        ? { value: count, tooltip: `${count} runtime item${count === 1 ? "" : "s"}` }
        : undefined;
  }

  getTreeItem(element: CoverageItem): vscode.TreeItem {
    return element;
  }

  getChildren(element?: CoverageItem): CoverageItem[] {
    if (element instanceof CoverageGroupItem) {
      return [...element.children];
    }

    if (!this.report) {
      return [];
    }

    const hotPaths = sortHotPaths(this.report).map(hotPathLeaf);
    const { safeToDelete, reviewRequired } = splitCleanupCandidates(this.report);

    const safeLeaves = safeToDelete.map((finding) =>
      findingLeaf(finding, "Candidate for deletion, verify before removing."),
    );
    const reviewLeaves = reviewRequired.map((finding) =>
      findingLeaf(finding, "Candidate flagged for review, runtime evidence is incomplete."),
    );

    const groups: CoverageItem[] = [];
    if (hotPaths.length > 0) {
      groups.push(new CoverageGroupItem("Hot Paths", "flame", hotPaths));
    }
    if (safeLeaves.length > 0) {
      groups.push(new CoverageGroupItem("Safe to Delete", "trash", safeLeaves));
    }
    if (reviewLeaves.length > 0) {
      groups.push(new CoverageGroupItem("Review Required", "eye", reviewLeaves));
    }
    return groups;
  }

  dispose(): void {
    this._onDidChangeTreeData.dispose();
  }
}
