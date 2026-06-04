// VS Code calls TreeDataProvider members through the registered provider.
// fallow-ignore-file unused-class-member
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { countSecurityFindings, hopRoleLabel, securityFindingLabel } from "./security-utils.js";
import { resolveFilePath as resolveFilePathPure } from "./treeView-utils.js";
import type { SecurityFinding, SecurityOutput, TraceHop } from "./types.js";

const resolveFilePath = (filePath: string | undefined): { absolute: string; relative: string } =>
  resolveFilePathPure(filePath, vscode.workspace.workspaceFolders?.[0]?.uri.fsPath);

/**
 * The candidate-framing prefix shown at the head of every finding tooltip. The
 * MUST of issue #903: every user-visible security surface reads as an unverified
 * candidate pending verification, never a confirmed vulnerability.
 */
const CANDIDATE_TOOLTIP_PREFIX = "UNVERIFIED CANDIDATE - verify before acting";

const openCommand = (absolute: string, line: number, col: number): vscode.Command => ({
  command: "vscode.open",
  title: "Open File",
  arguments: [
    vscode.Uri.file(absolute),
    {
      selection: new vscode.Range(Math.max(0, line - 1), col, Math.max(0, line - 1), col),
    },
  ],
});

const reachabilityLine = (finding: SecurityFinding): string | null => {
  const reach = finding.reachability;
  if (!reach) {
    return null;
  }
  const entry = reach.reachable_from_entry
    ? "reachable from a runtime entry point"
    : "not reached from any runtime entry point";
  const boundary = reach.crosses_boundary ? "; crosses an architecture boundary" : "";
  return `reach: ${entry} (blast radius ${reach.blast_radius})${boundary}`;
};

type SecurityItem = SecurityFindingItem | SecurityHopItem;

class SecurityHopItem extends vscode.TreeItem {
  constructor(hop: TraceHop) {
    const { absolute, relative } = resolveFilePath(hop.path);
    super(`${relative}:${hop.line}`, vscode.TreeItemCollapsibleState.None);

    this.description = hopRoleLabel(hop.role);
    this.tooltip = `${hopRoleLabel(hop.role)}\n${absolute}:${hop.line}`;
    this.contextValue = "securityHop";
    this.iconPath = new vscode.ThemeIcon("arrow-small-right");
    this.command = openCommand(absolute, hop.line, hop.col);
  }
}

class SecurityFindingItem extends vscode.TreeItem {
  readonly hops: ReadonlyArray<SecurityHopItem>;

  constructor(finding: SecurityFinding) {
    const label = securityFindingLabel(finding);
    const hops = finding.trace.map((hop) => new SecurityHopItem(hop));
    super(
      label,
      hops.length > 0
        ? vscode.TreeItemCollapsibleState.Collapsed
        : vscode.TreeItemCollapsibleState.None,
    );

    const { absolute, relative } = resolveFilePath(finding.path);

    this.description = `${relative}:${finding.line}`;
    this.contextValue = "securityCandidate";
    this.iconPath = new vscode.ThemeIcon("shield");
    this.hops = hops;

    const tooltipLines = [
      CANDIDATE_TOOLTIP_PREFIX,
      "",
      finding.evidence,
      `${absolute}:${finding.line}:${finding.col}`,
    ];
    const reach = reachabilityLine(finding);
    if (reach) {
      tooltipLines.push(reach);
    }
    this.tooltip = tooltipLines.join("\n");

    this.command = openCommand(absolute, finding.line, finding.col);
  }
}

/**
 * Renders local security CANDIDATES from `fallow security` into the Security
 * Candidates view. Structured like `DuplicatesTreeProvider`: a flat list of
 * findings, each with collapsible trace hops. Every node frames the finding as
 * unverified (#903): the view name, the tooltip prefix, and the toast wording
 * make clear these are candidates to verify, not confirmed vulnerabilities.
 */
export class SecurityTreeProvider implements vscode.TreeDataProvider<SecurityItem> {
  private result: SecurityOutput | null = null;
  private view: vscode.TreeView<SecurityItem> | null = null;

  private readonly _onDidChangeTreeData = new vscode.EventEmitter<
    SecurityItem | undefined | null | void
  >();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  setView(view: vscode.TreeView<SecurityItem>): void {
    this.view = view;
  }

  update(result: SecurityOutput | null): void {
    this.result = result;
    this._onDidChangeTreeData.fire();
    this.updateBadge();
  }

  private updateBadge(): void {
    if (!this.view) {
      return;
    }
    const count = countSecurityFindings(this.result);
    this.view.badge =
      count > 0
        ? {
            value: count,
            tooltip: `${count} security candidate${count === 1 ? "" : "s"} to verify`,
          }
        : undefined;
  }

  getTreeItem(element: SecurityItem): vscode.TreeItem {
    return element;
  }

  getChildren(element?: SecurityItem): SecurityItem[] {
    if (element instanceof SecurityFindingItem) {
      return [...element.hops];
    }

    if (!this.result) {
      return [];
    }

    return this.result.security_findings.map((finding) => new SecurityFindingItem(finding));
  }

  dispose(): void {
    this._onDidChangeTreeData.dispose();
  }
}
