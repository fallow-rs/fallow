// VS Code calls TreeDataProvider members through the registered provider.
// fallow-ignore-file unused-class-member
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  countHealthItems,
  escapeHealthMarkdown,
  formatComplexityOffense,
  formatHotspotDescription,
  formatScoreLabel,
  gradeIcon,
  gradeThemeColor,
  severityIcon,
  severityThemeColor,
  topPenalties,
} from "./health-utils.js";
import { getHealthTopFindings } from "./config.js";
import { HEALTH_SECTION_ICONS, HEALTH_SECTION_LABELS } from "./health-labels.js";
import type { HealthSection } from "./health-labels.js";
import { middleElidePath, resolveFilePath as resolveFilePathPure } from "./treeView-utils.js";
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
    super(count > 0 ? `${base} (${count})` : base, vscode.TreeItemCollapsibleState.Collapsed);
    this.contextValue = `healthSection.${section}`;
    this.iconPath = new vscode.ThemeIcon(HEALTH_SECTION_ICONS[section]);
  }
}

/**
 * A health row. File-bearing rows render as two lines: the file is the parent
 * (auto-expanded) and the detail (metrics / recommendation) is a single child,
 * which carries the open-on-click command. The Score row is a childless leaf
 * with no command. Pass `icon: undefined` for an icon-less child detail row.
 */
class HealthLeafItem extends vscode.TreeItem {
  readonly children: ReadonlyArray<HealthLeafItem>;
  /** Set on complexity rows so Health-view selection can expand the function. */
  readonly complexityTarget?: { readonly path: string; readonly line: number };

  constructor(
    label: string,
    icon: string | undefined,
    options: {
      readonly tooltip?: string | vscode.MarkdownString;
      readonly iconColor?: string | null;
      readonly open?: { readonly path: string; readonly line: number; readonly col: number };
      readonly complexityTarget?: { readonly path: string; readonly line: number };
      readonly children?: ReadonlyArray<HealthLeafItem>;
    } = {},
  ) {
    const children = options.children ?? [];
    super(
      label,
      children.length > 0
        ? vscode.TreeItemCollapsibleState.Expanded
        : vscode.TreeItemCollapsibleState.None,
    );
    this.children = children;
    this.contextValue = "healthItem";
    if (options.complexityTarget) {
      this.complexityTarget = options.complexityTarget;
    }

    if (options.tooltip !== undefined) {
      this.tooltip = options.tooltip;
    }

    if (icon !== undefined) {
      const color =
        options.iconColor != null ? new vscode.ThemeColor(options.iconColor) : undefined;
      this.iconPath = new vscode.ThemeIcon(icon, color);
    }

    if (options.open) {
      const { absolute } = resolveFilePath(options.open.path);
      const line = Math.max(0, options.open.line - 1);
      const col = Math.max(0, options.open.col);
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

/**
 * The complex function a Health-view item points at, if any, so the editor can
 * expand that function's inline breakdown while the row is selected. Returns
 * `undefined` for section headers, the Score row, hotspots, and targets.
 */
export const complexityTargetOf = (
  item: vscode.TreeItem,
): { readonly path: string; readonly line: number } | undefined =>
  item instanceof HealthLeafItem ? item.complexityTarget : undefined;

const buildScoreTooltip = (report: HealthReport): vscode.MarkdownString => {
  const score = report.health_score;
  const md = new vscode.MarkdownString();
  md.supportThemeIcons = true;
  if (!score) {
    md.appendMarkdown("Project health score (run with `--score`).");
    return md;
  }
  // Round to a whole number so the tooltip header matches the tree row label
  // (`formatScoreLabel` rounds too); a one-decimal header read inconsistently
  // next to the rounded row.
  const roundedScore = Number.isFinite(score.score) ? Math.round(score.score) : 0;
  const safeGrade = escapeHealthMarkdown(score.grade.trim() || "?");
  md.appendMarkdown(`**Health score:** ${roundedScore} (grade ${safeGrade})\n\n`);
  const penalties = topPenalties(score.penalties);
  if (penalties.length > 0) {
    md.appendMarkdown("Top penalty contributors:\n\n");
    for (const penalty of penalties) {
      md.appendMarkdown(`- ${escapeHealthMarkdown(penalty.key)}: -${penalty.points.toFixed(1)}\n`);
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

// Each file-bearing Health row is two lines: the file is the parent (auto-
// expanded) and the detail (metrics / recommendation) is the indented child
// that carries the open-on-click command.

// The health spawn may fetch more findings than the tree shows (a higher
// `--top` so the inline editor breakdown can decorate files outside the tree's
// top-N). The tree still displays only `health.topFindings`.
const visibleComplexityFindings = (report: HealthReport): HealthReport["findings"] =>
  (report.findings ?? []).slice(0, getHealthTopFindings());

const buildComplexityLeaves = (report: HealthReport): HealthLeafItem[] =>
  visibleComplexityFindings(report).map((finding) => {
    const { relative } = resolveFilePath(finding.path);
    const crapNote = typeof finding.crap === "number" ? `, CRAP ${finding.crap.toFixed(0)}` : "";
    const tooltip = `${finding.name} (${finding.severity})\ncyclomatic ${finding.cyclomatic}, cognitive ${finding.cognitive}${crapNote}\n${relative}:${finding.line}`;
    const complexityTarget = { path: finding.path, line: finding.line };
    const detail = new HealthLeafItem(formatComplexityOffense(finding), undefined, {
      tooltip,
      open: { path: finding.path, line: finding.line, col: finding.col },
      complexityTarget,
    });
    return new HealthLeafItem(
      `${middleElidePath(relative)}:${finding.line}`,
      severityIcon(finding.severity),
      {
        iconColor: severityThemeColor(finding.severity),
        tooltip,
        complexityTarget,
        children: [detail],
      },
    );
  });

const buildHotspotLeaves = (report: HealthReport): HealthLeafItem[] =>
  (report.hotspots ?? []).map((hotspot) => {
    const { relative } = resolveFilePath(hotspot.path);
    const tooltip = new vscode.MarkdownString();
    tooltip.appendMarkdown(
      `**${escapeHealthMarkdown(relative)}**\n\nChurn x complexity hotspot (score ${hotspot.score.toFixed(1)}, ${hotspot.commits} commit${hotspot.commits === 1 ? "" : "s"}).\n\n_Heuristic candidate, verify before acting._`,
    );
    const detail = new HealthLeafItem(
      formatHotspotDescription(hotspot.score, hotspot.commits),
      undefined,
      { tooltip, open: { path: hotspot.path, line: 1, col: 0 } },
    );
    return new HealthLeafItem(middleElidePath(relative), "git-commit", {
      tooltip,
      children: [detail],
    });
  });

const buildTargetLeaves = (report: HealthReport): HealthLeafItem[] =>
  (report.targets ?? []).map((target) => {
    const { relative } = resolveFilePath(target.path);
    const tooltip = new vscode.MarkdownString();
    tooltip.appendMarkdown(
      `**${escapeHealthMarkdown(target.recommendation)}**\n\nEffort: ${escapeHealthMarkdown(target.effort)}, Confidence: ${escapeHealthMarkdown(target.confidence)}, Priority: ${target.priority.toFixed(0)}\n\n${escapeHealthMarkdown(relative)}\n\n_Heuristic suggestion, verify before acting._`,
    );
    const detail = new HealthLeafItem(target.recommendation, undefined, {
      tooltip,
      open: { path: target.path, line: 1, col: 0 },
    });
    return new HealthLeafItem(middleElidePath(relative), "tools", {
      tooltip,
      children: [detail],
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

    if (element instanceof HealthLeafItem) {
      return [...element.children];
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
    addSection(
      "complexity",
      buildComplexityLeaves(this.report),
      visibleComplexityFindings(this.report).length,
    );
    addSection("hotspots", buildHotspotLeaves(this.report), this.report.hotspots?.length ?? 0);
    addSection("targets", buildTargetLeaves(this.report), this.report.targets?.length ?? 0);

    return sections;
  }

  dispose(): void {
    this._onDidChangeTreeData.dispose();
  }
}
