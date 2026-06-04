// VS Code calls TreeDataProvider members through the registered provider.
// fallow-ignore-file unused-class-member
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  countHealthItems,
  formatHotspotDescription,
  formatScoreLabel,
  gradeIcon,
  gradeThemeColor,
  severityIcon,
  topPenalties,
} from "./health-utils.js";
import { HEALTH_SECTION_ICONS, HEALTH_SECTION_LABELS } from "./health-labels.js";
import type { HealthSection } from "./health-labels.js";
import { resolveFilePath as resolveFilePathPure } from "./treeView-utils.js";
import type { HealthReport } from "./types.js";

const resolveFilePath = (filePath: string | undefined) =>
  resolveFilePathPure(filePath, vscode.workspace.workspaceFolders?.[0]?.uri.fsPath);

type HealthItem = HealthSectionItem | HealthLeafItem;

/** A collapsible section header (Score / Complexity / Hotspots / Targets). */
class HealthSectionItem extends vscode.TreeItem {
  constructor(
    readonly section: HealthSection,
    readonly leaves: ReadonlyArray<HealthLeafItem>,
    count: number,
  ) {
    const base = HEALTH_SECTION_LABELS[section];
    super(
      count > 0 ? `${base} (${count})` : base,
      vscode.TreeItemCollapsibleState.Collapsed,
    );
    this.contextValue = `healthSection.${section}`;
    this.iconPath = new vscode.ThemeIcon(HEALTH_SECTION_ICONS[section]);
  }
}

/**
 * A leaf row. Optionally opens a file:line on click (complexity findings,
 * hotspots, targets all carry a path). The Score row is a leaf with no command.
 */
class HealthLeafItem extends vscode.TreeItem {
  constructor(
    label: string,
    icon: string,
    options: {
      readonly description?: string;
      readonly tooltip?: string | vscode.MarkdownString;
      readonly iconColor?: string | null;
      readonly open?: { readonly path: string; readonly line: number; readonly col: number };
    } = {},
  ) {
    super(label, vscode.TreeItemCollapsibleState.None);
    this.contextValue = "healthItem";

    if (options.description !== undefined) {
      this.description = options.description;
    }
    if (options.tooltip !== undefined) {
      this.tooltip = options.tooltip;
    }

    const color =
      options.iconColor != null ? new vscode.ThemeColor(options.iconColor) : undefined;
    this.iconPath = new vscode.ThemeIcon(icon, color);

    if (options.open) {
      const { absolute, relative } = resolveFilePath(options.open.path);
      const line = Math.max(0, options.open.line - 1);
      const col = Math.max(0, options.open.col);
      if (options.description === undefined) {
        this.description = `${relative}:${options.open.line}`;
      }
      this.command = {
        command: "vscode.open",
        title: "Open File",
        arguments: [
          vscode.Uri.file(absolute),
          { selection: new vscode.Range(line, col, line, col) },
        ],
      };
    }
  }
}

const buildScoreTooltip = (report: HealthReport): vscode.MarkdownString => {
  const score = report.health_score;
  const md = new vscode.MarkdownString();
  md.supportThemeIcons = true;
  if (!score) {
    md.appendMarkdown("Project health score (run with `--score`).");
    return md;
  }
  md.appendMarkdown(
    `**Health score:** ${score.score.toFixed(1)} (grade ${score.grade.trim() || "?"})\n\n`,
  );
  const penalties = topPenalties(score.penalties);
  if (penalties.length > 0) {
    md.appendMarkdown("Top penalty contributors:\n\n");
    for (const penalty of penalties) {
      md.appendMarkdown(`- ${penalty.key}: -${penalty.points.toFixed(1)}\n`);
    }
  } else {
    md.appendMarkdown("No penalties applied.");
  }
  return md;
};

const buildScoreLeaves = (report: HealthReport): HealthLeafItem[] => {
  const score = report.health_score;
  if (!score) {
    return [];
  }
  return [
    new HealthLeafItem(formatScoreLabel(score.score, score.grade), gradeIcon(score.grade), {
      iconColor: gradeThemeColor(score.grade),
      tooltip: buildScoreTooltip(report),
    }),
  ];
};

const buildComplexityLeaves = (report: HealthReport): HealthLeafItem[] =>
  (report.findings ?? []).map((finding) => {
    const crapNote =
      typeof finding.crap === "number" ? `, CRAP ${finding.crap.toFixed(0)}` : "";
    const tooltip = `${finding.name} (${finding.severity}): cyclomatic ${finding.cyclomatic}, cognitive ${finding.cognitive}${crapNote}`;
    return new HealthLeafItem(finding.name, severityIcon(finding.severity), {
      tooltip,
      open: { path: finding.path, line: finding.line, col: finding.col },
    });
  });

const buildHotspotLeaves = (report: HealthReport): HealthLeafItem[] =>
  (report.hotspots ?? []).map((hotspot) => {
    const { relative } = resolveFilePath(hotspot.path);
    const tooltip = new vscode.MarkdownString();
    tooltip.appendMarkdown(
      `**${relative}**\n\nChurn x complexity hotspot (score ${hotspot.score.toFixed(1)}, ${hotspot.commits} commit${hotspot.commits === 1 ? "" : "s"}).\n\n_Heuristic candidate, verify before acting._`,
    );
    return new HealthLeafItem(relative, "git-commit", {
      description: formatHotspotDescription(hotspot.score, hotspot.commits),
      tooltip,
      open: { path: hotspot.path, line: 1, col: 0 },
    });
  });

const buildTargetLeaves = (report: HealthReport): HealthLeafItem[] =>
  (report.targets ?? []).map((target) => {
    const { relative } = resolveFilePath(target.path);
    const tooltip = new vscode.MarkdownString();
    tooltip.appendMarkdown(
      `**${target.recommendation}**\n\nEffort: ${target.effort}, Confidence: ${target.confidence}, Priority: ${target.priority.toFixed(0)}\n\n_Heuristic suggestion, verify before acting._`,
    );
    return new HealthLeafItem(target.recommendation, "tools", {
      description: relative,
      tooltip,
      open: { path: target.path, line: 1, col: 0 },
    });
  });

export class HealthTreeProvider implements vscode.TreeDataProvider<HealthItem> {
  private report: HealthReport | null = null;
  private view: vscode.TreeView<HealthItem> | null = null;

  private readonly _onDidChangeTreeData = new vscode.EventEmitter<
    HealthItem | undefined | null | void
  >();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  setView(view: vscode.TreeView<HealthItem>): void {
    this.view = view;
  }

  update(report: HealthReport | null): void {
    this.report = report;
    this._onDidChangeTreeData.fire();
    this.updateBadge();
  }

  private updateBadge(): void {
    if (!this.view) {
      return;
    }
    const count = countHealthItems(this.report);
    this.view.badge =
      count > 0
        ? { value: count, tooltip: `${count} health item${count === 1 ? "" : "s"}` }
        : undefined;
  }

  getTreeItem(element: HealthItem): vscode.TreeItem {
    return element;
  }

  getChildren(element?: HealthItem): HealthItem[] {
    if (element instanceof HealthSectionItem) {
      return [...element.leaves];
    }

    if (!this.report) {
      return [];
    }

    const sections: HealthItem[] = [];
    const addSection = (
      section: HealthSection,
      leaves: ReadonlyArray<HealthLeafItem>,
      count: number,
    ): void => {
      if (leaves.length > 0) {
        sections.push(new HealthSectionItem(section, leaves, count));
      }
    };

    // The score is a single summary row, so suppress the redundant "(1)" count.
    addSection("score", buildScoreLeaves(this.report), 0);
    addSection("complexity", buildComplexityLeaves(this.report), this.report.findings?.length ?? 0);
    addSection("hotspots", buildHotspotLeaves(this.report), this.report.hotspots?.length ?? 0);
    addSection("targets", buildTargetLeaves(this.report), this.report.targets?.length ?? 0);

    return sections;
  }

  dispose(): void {
    this._onDidChangeTreeData.dispose();
  }
}
