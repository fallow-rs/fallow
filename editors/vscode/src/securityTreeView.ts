// VS Code calls TreeDataProvider members through the registered provider.
// fallow-ignore-file unused-class-member
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { countSecurityFindings, hopRoleLabel, securityFindingLabel } from "./security-utils.js";
import { middleElidePath, resolveFilePath as resolveFilePathPure } from "./treeView-utils.js";
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
  const source = reach.reachable_from_untrusted_source
    ? `; module reachable from an untrusted-source module via ${pluralize(reach.untrusted_source_hop_count ?? 0, "import hop", "import hops")}`
    : "";
  const boundary = reach.crosses_boundary ? "; crosses an architecture boundary" : "";
  return `reach: ${entry} (blast radius ${reach.blast_radius})${source}${boundary}`;
};

const pluralize = (count: number, singular: string, plural: string): string =>
  `${count} ${count === 1 ? singular : plural}`;

const untrustedSourceTraceLines = (finding: SecurityFinding): string[] => {
  const trace = finding.reachability?.untrusted_source_trace ?? [];
  if (trace.length === 0) {
    return [];
  }
  return [
    "untrusted-source trace:",
    ...trace.map((hop) => {
      const { relative } = resolveFilePath(hop.path);
      return `${hopRoleLabel(hop.role)} ${middleElidePath(relative)}:${hop.line}`;
    }),
  ];
};

type SecurityItem =
  | SecurityGroupItem
  | SecurityFindingItem
  | SecurityHopItem
  | SecurityBlindSpotItem;

class SecurityGroupItem extends vscode.TreeItem {
  readonly findings: ReadonlyArray<SecurityFindingItem>;

  constructor(
    readonly groupLabel: string,
    findings: ReadonlyArray<SecurityFindingItem>,
  ) {
    super(`${groupLabel} (${findings.length})`, vscode.TreeItemCollapsibleState.Collapsed);
    this.findings = findings;
    this.description = "security candidates to verify";
    this.tooltip = [CANDIDATE_TOOLTIP_PREFIX, "", `${groupLabel}: ${findings.length}`].join("\n");
    this.contextValue = "securityCandidateGroup";
    this.iconPath = new vscode.ThemeIcon("shield");
  }
}

class SecurityHopItem extends vscode.TreeItem {
  constructor(hop: TraceHop) {
    const { absolute, relative } = resolveFilePath(hop.path);
    super(`${middleElidePath(relative)}:${hop.line}`, vscode.TreeItemCollapsibleState.None);

    this.description = hopRoleLabel(hop.role);
    this.tooltip = [
      CANDIDATE_TOOLTIP_PREFIX,
      "",
      hopRoleLabel(hop.role),
      `${absolute}:${hop.line}`,
    ].join("\n");
    this.contextValue = "securityHop";
    this.iconPath = new vscode.ThemeIcon("arrow-small-right");
    this.command = openCommand(absolute, hop.line, hop.col);
  }
}

class SecurityBlindSpotItem extends vscode.TreeItem {
  constructor(result: SecurityOutput) {
    const edgeFiles = result.unresolved_edge_files;
    const calleeSites = result.unresolved_callee_sites;
    const label = `Blind spots: ${pluralize(edgeFiles, "unresolved import edge", "unresolved import edges")}, ${pluralize(calleeSites, "unresolved sink site", "unresolved sink sites")}`;
    super(label, vscode.TreeItemCollapsibleState.None);

    this.description = "not a clean bill of health";
    this.tooltip = [
      CANDIDATE_TOOLTIP_PREFIX,
      "",
      `${pluralize(edgeFiles, "unresolved import edge", "unresolved import edges")} not analyzed`,
      `${pluralize(calleeSites, "unresolved sink site", "unresolved sink sites")} not analyzed`,
      "An empty result is not a clean bill of health.",
    ].join("\n");
    this.contextValue = "securityBlindSpot";
    this.iconPath = new vscode.ThemeIcon("info");
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

    this.description = `${middleElidePath(relative)}:${finding.line}`;
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
    tooltipLines.push(...untrustedSourceTraceLines(finding));
    this.tooltip = tooltipLines.join("\n");

    this.command = openCommand(absolute, finding.line, finding.col);
  }
}

/**
 * Renders local security CANDIDATES from `fallow security` into the Security
 * Candidates view. Findings are grouped by kind and CWE/category, then each
 * finding can expand into detector trace hops, while source-reachability traces
 * appear in the finding tooltip. Every node frames the finding as unverified
 * (#903): the view name, the tooltip prefix, and the toast wording make clear
 * these are candidates to verify, not confirmed vulnerabilities.
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
    if (element instanceof SecurityGroupItem) {
      return [...element.findings];
    }

    if (element instanceof SecurityFindingItem) {
      return [...element.hops];
    }

    if (element) {
      return [];
    }

    if (!this.result) {
      return [];
    }

    const groups = new Map<string, SecurityFindingItem[]>();
    for (const finding of this.result.security_findings) {
      const label = securityFindingLabel(finding);
      const existing = groups.get(label);
      if (existing) {
        existing.push(new SecurityFindingItem(finding));
      } else {
        groups.set(label, [new SecurityFindingItem(finding)]);
      }
    }

    const items: SecurityItem[] = [...groups.entries()].map(
      ([label, findings]) => new SecurityGroupItem(label, findings),
    );

    if (this.result.unresolved_edge_files > 0 || this.result.unresolved_callee_sites > 0) {
      items.push(new SecurityBlindSpotItem(this.result));
    }

    return items;
  }

  dispose(): void {
    this._onDidChangeTreeData.dispose();
  }
}
