import * as path from "node:path";
// VS Code calls TreeDataProvider members through the registered provider.
// fallow-ignore-file unused-class-member
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { countCheckIssues } from "./analysis-utils.js";
import { resolveFilePath as resolveFilePathPure } from "./treeView-utils.js";
import type {
  CloneGroup,
  FallowCheckResult,
  FallowDupesResult,
  IssueCategory,
} from "./types.js";
import { ISSUE_CATEGORY_LABELS } from "./types.js";

const resolveFilePath = (filePath: string | undefined) =>
  resolveFilePathPure(filePath, vscode.workspace.workspaceFolders?.[0]?.uri.fsPath);

/** Icons per issue category. */
const CATEGORY_ICONS: Record<IssueCategory, string> = {
  "unused-files": "file-code",
  "unused-exports": "symbol-method",
  "unused-types": "symbol-interface",
  "private-type-leaks": "symbol-interface",
  "unused-dependencies": "package",
  "unused-dev-dependencies": "package",
  "unused-optional-dependencies": "package",
  "unused-enum-members": "symbol-enum-member",
  "unused-class-members": "symbol-field",
  "unresolved-imports": "error",
  "unlisted-dependencies": "package",
  "duplicate-exports": "files",
  "type-only-dependencies": "symbol-interface",
  "test-only-dependencies": "beaker",
  "circular-dependencies": "sync",
  "boundary-violation": "symbol-namespace",
  "stale-suppressions": "trash",
  "unused-catalog-entries": "package",
  "unresolved-catalog-references": "error",
  "unused-dependency-overrides": "package",
  "misconfigured-dependency-overrides": "error",
};

/** Icons for individual issue items. */
const ISSUE_ICONS: Record<IssueCategory, string> = {
  "unused-files": "file",
  "unused-exports": "symbol-method",
  "unused-types": "symbol-interface",
  "private-type-leaks": "symbol-interface",
  "unused-dependencies": "package",
  "unused-dev-dependencies": "package",
  "unused-optional-dependencies": "package",
  "unused-enum-members": "symbol-enum-member",
  "unused-class-members": "symbol-field",
  "unresolved-imports": "error",
  "unlisted-dependencies": "package",
  "duplicate-exports": "copy",
  "type-only-dependencies": "package",
  "test-only-dependencies": "beaker",
  "circular-dependencies": "sync",
  "boundary-violation": "symbol-namespace",
  "stale-suppressions": "trash",
  "unused-catalog-entries": "package",
  "unresolved-catalog-references": "error",
  "unused-dependency-overrides": "package",
  "misconfigured-dependency-overrides": "error",
};

const staleSuppressionLabel = (
  origin: NonNullable<FallowCheckResult["stale_suppressions"]>[number]["origin"]
): string => {
  if (origin.type === "jsdoc_tag") {
    return `@expected-unused ${origin.export_name}`;
  }
  if (origin.issue_kind) {
    return origin.is_file_level
      ? `file ${origin.issue_kind}`
      : origin.issue_kind;
  }
  return origin.is_file_level ? "file suppression" : "line suppression";
};

type DeadCodeItem = CategoryItem | IssueItem;

class CategoryItem extends vscode.TreeItem {
  readonly issues: ReadonlyArray<IssueItem>;

  constructor(
    readonly category: IssueCategory,
    issues: ReadonlyArray<IssueItem>
  ) {
    super(
      `${ISSUE_CATEGORY_LABELS[category]} (${issues.length})`,
      vscode.TreeItemCollapsibleState.Collapsed
    );
    this.issues = issues;
    this.contextValue = "category";
    this.iconPath = new vscode.ThemeIcon(CATEGORY_ICONS[category] ?? "warning");
  }
}

class IssueItem extends vscode.TreeItem {
  constructor(
    label: string,
    readonly filePath: string,
    readonly line: number,
    readonly col: number,
    category: IssueCategory
  ) {
    super(label, vscode.TreeItemCollapsibleState.None);

    const { absolute, relative } = resolveFilePath(filePath);

    this.description = `${relative}:${line}`;
    this.tooltip = `${label}\n${absolute}:${line}:${col}`;
    this.contextValue = "issue";

    this.command = {
      command: "vscode.open",
      title: "Open File",
      arguments: [
        vscode.Uri.file(absolute),
        {
          selection: new vscode.Range(
            Math.max(0, line - 1),
            col,
            Math.max(0, line - 1),
            col
          ),
        },
      ],
    };

    this.iconPath = new vscode.ThemeIcon(ISSUE_ICONS[category] ?? "warning");
  }
}

export class DeadCodeTreeProvider
  implements vscode.TreeDataProvider<DeadCodeItem>
{
  private result: FallowCheckResult | null = null;
  private view: vscode.TreeView<DeadCodeItem> | null = null;

  private readonly _onDidChangeTreeData = new vscode.EventEmitter<
    DeadCodeItem | undefined | null | void
  >();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  setView(view: vscode.TreeView<DeadCodeItem>): void {
    this.view = view;
  }

  update(result: FallowCheckResult | null): void {
    this.result = result;
    this._onDidChangeTreeData.fire();
    this.updateBadge();
  }

  private updateBadge(): void {
    if (!this.view) {
      return;
    }
    if (!this.result) {
      this.view.badge = undefined;
      return;
    }
    const count = countCheckIssues(this.result);

    this.view.badge = count > 0
      ? { value: count, tooltip: `${count} issue${count === 1 ? "" : "s"}` }
      : undefined;
  }

  getTreeItem(element: DeadCodeItem): vscode.TreeItem {
    return element;
  }

  getChildren(element?: DeadCodeItem): DeadCodeItem[] {
    if (element instanceof CategoryItem) {
      return [...element.issues];
    }

    if (!this.result) {
      return [];
    }

    const categories: DeadCodeItem[] = [];

    const addCategory = (
      category: IssueCategory,
      items: ReadonlyArray<IssueItem>
    ): void => {
      if (items.length > 0) {
        categories.push(new CategoryItem(category, items));
      }
    };

    addCategory(
      "unused-files",
      this.result.unused_files.map(
        (f) => new IssueItem(path.basename(f.path), f.path, 1, 0, "unused-files")
      )
    );

    addCategory(
      "unused-exports",
      this.result.unused_exports.map(
        (e) => new IssueItem(e.export_name, e.path, e.line, e.col, "unused-exports")
      )
    );

    addCategory(
      "unused-types",
      this.result.unused_types.map(
        (e) => new IssueItem(e.export_name, e.path, e.line, e.col, "unused-types")
      )
    );

    addCategory(
      "private-type-leaks",
      (this.result.private_type_leaks ?? []).map(
        (l) =>
          new IssueItem(
            `${l.export_name} -> ${l.type_name}`,
            l.path,
            l.line,
            l.col,
            "private-type-leaks"
          )
      )
    );

    addCategory(
      "unused-dependencies",
      this.result.unused_dependencies.map(
        (d) => new IssueItem(d.package_name, d.path, d.line, 0, "unused-dependencies")
      )
    );

    addCategory(
      "unused-dev-dependencies",
      this.result.unused_dev_dependencies.map(
        (d) => new IssueItem(d.package_name, d.path, d.line, 0, "unused-dev-dependencies")
      )
    );

    if (this.result.unused_optional_dependencies) {
      addCategory(
        "unused-optional-dependencies",
        this.result.unused_optional_dependencies.map(
          (d) => new IssueItem(d.package_name, d.path, d.line, 0, "unused-optional-dependencies")
        )
      );
    }

    addCategory(
      "unused-enum-members",
      this.result.unused_enum_members.map(
        (m) =>
          new IssueItem(`${m.parent_name}.${m.member_name}`, m.path, m.line, m.col, "unused-enum-members")
      )
    );

    addCategory(
      "unused-class-members",
      this.result.unused_class_members.map(
        (m) =>
          new IssueItem(`${m.parent_name}.${m.member_name}`, m.path, m.line, m.col, "unused-class-members")
      )
    );

    addCategory(
      "unresolved-imports",
      this.result.unresolved_imports.map(
        (i) => new IssueItem(i.specifier, i.path, i.line, i.col, "unresolved-imports")
      )
    );

    addCategory(
      "unlisted-dependencies",
      this.result.unlisted_dependencies.flatMap((d) =>
        d.imported_from.map(
          (site) => new IssueItem(d.package_name, site.path, site.line, site.col, "unlisted-dependencies")
        )
      )
    );

    addCategory(
      "duplicate-exports",
      this.result.duplicate_exports.flatMap((d) =>
        d.locations.map(
          (loc) => new IssueItem(d.export_name, loc.path, loc.line, loc.col, "duplicate-exports")
        )
      )
    );

    if (this.result.type_only_dependencies) {
      addCategory(
        "type-only-dependencies",
        this.result.type_only_dependencies.map(
          (d) => new IssueItem(d.package_name, d.path, d.line, 0, "type-only-dependencies")
        )
      );
    }

    if (this.result.test_only_dependencies) {
      addCategory(
        "test-only-dependencies",
        this.result.test_only_dependencies.map(
          (d) => new IssueItem(d.package_name, d.path, d.line, 0, "test-only-dependencies")
        )
      );
    }

    if (this.result.circular_dependencies) {
      addCategory(
        "circular-dependencies",
        this.result.circular_dependencies.map(
          (c) => new IssueItem(
            `${c.length} files`,
            c.files[0] ?? "",
            c.line,
            c.col,
            "circular-dependencies"
          )
        )
      );
    }

    if (this.result.boundary_violations) {
      addCategory(
        "boundary-violation",
        this.result.boundary_violations.map(
          (v) =>
            new IssueItem(
              `${v.from_zone} -> ${v.to_zone}`,
              v.from_path,
              v.line,
              v.col,
              "boundary-violation"
            )
        )
      );
    }

    if (this.result.stale_suppressions) {
      addCategory(
        "stale-suppressions",
        this.result.stale_suppressions.map(
          (s) =>
            new IssueItem(
              staleSuppressionLabel(s.origin),
              s.path,
              s.line,
              s.col,
              "stale-suppressions"
            )
        )
      );
    }

    if (this.result.unused_catalog_entries) {
      addCategory(
        "unused-catalog-entries",
        this.result.unused_catalog_entries.map(
          (entry) =>
            new IssueItem(
              entry.catalog_name === "default"
                ? entry.entry_name
                : `${entry.entry_name} (${entry.catalog_name})`,
              entry.path,
              entry.line,
              0,
              "unused-catalog-entries"
            )
        )
      );
    }

    if (this.result.unresolved_catalog_references) {
      addCategory(
        "unresolved-catalog-references",
        this.result.unresolved_catalog_references.map(
          (finding) =>
            new IssueItem(
              finding.catalog_name === "default"
                ? finding.entry_name
                : `${finding.entry_name} (${finding.catalog_name})`,
              finding.path,
              finding.line,
              0,
              "unresolved-catalog-references"
            )
        )
      );
    }

    if (this.result.unused_dependency_overrides) {
      addCategory(
        "unused-dependency-overrides",
        this.result.unused_dependency_overrides.map(
          (finding) =>
            new IssueItem(
              `${finding.raw_key} (${finding.source})`,
              finding.path,
              finding.line,
              0,
              "unused-dependency-overrides"
            )
        )
      );
    }

    if (this.result.misconfigured_dependency_overrides) {
      addCategory(
        "misconfigured-dependency-overrides",
        this.result.misconfigured_dependency_overrides.map(
          (finding) =>
            new IssueItem(
              `${finding.raw_key} (${finding.source})`,
              finding.path,
              finding.line,
              0,
              "misconfigured-dependency-overrides"
            )
        )
      );
    }

    return categories;
  }

  dispose(): void {
    this._onDidChangeTreeData.dispose();
  }
}

type DuplicateItem = CloneFamilyItem | CloneInstanceItem;

class CloneFamilyItem extends vscode.TreeItem {
  readonly instances: ReadonlyArray<CloneInstanceItem>;

  constructor(group: CloneGroup, index: number) {
    const instanceItems = group.instances.map(
      (inst) => new CloneInstanceItem(inst.file, inst.start_line, inst.end_line)
    );
    super(
      `Clone #${index + 1} (${group.line_count} lines, ${group.instances.length} instances)`,
      vscode.TreeItemCollapsibleState.Collapsed
    );
    this.instances = instanceItems;
    this.contextValue = "cloneFamily";
    this.iconPath = new vscode.ThemeIcon("files");
  }
}

class CloneInstanceItem extends vscode.TreeItem {
  constructor(
    readonly filePath: string,
    readonly startLine: number,
    readonly endLine: number
  ) {
    const basename = path.basename(filePath);
    super(
      `${basename}:${startLine}-${endLine}`,
      vscode.TreeItemCollapsibleState.None
    );

    const { absolute, relative } = resolveFilePath(filePath);

    this.description = relative;
    this.tooltip = `${absolute}:${startLine}-${endLine}`;
    this.contextValue = "cloneInstance";

    this.command = {
      command: "vscode.open",
      title: "Open File",
      arguments: [
        vscode.Uri.file(absolute),
        {
          selection: new vscode.Range(
            Math.max(0, startLine - 1),
            0,
            Math.max(0, endLine - 1),
            0
          ),
        },
      ],
    };

    this.iconPath = new vscode.ThemeIcon("copy");
  }
}

export class DuplicatesTreeProvider
  implements vscode.TreeDataProvider<DuplicateItem>
{
  private result: FallowDupesResult | null = null;

  private readonly _onDidChangeTreeData = new vscode.EventEmitter<
    DuplicateItem | undefined | null | void
  >();
  readonly onDidChangeTreeData = this._onDidChangeTreeData.event;

  update(result: FallowDupesResult | null): void {
    this.result = result;
    this._onDidChangeTreeData.fire();
  }

  getTreeItem(element: DuplicateItem): vscode.TreeItem {
    return element;
  }

  getChildren(element?: DuplicateItem): DuplicateItem[] {
    if (element instanceof CloneFamilyItem) {
      return [...element.instances];
    }

    if (!this.result) {
      return [];
    }

    return this.result.clone_groups.map(
      (group, i) => new CloneFamilyItem(group, i)
    );
  }

  dispose(): void {
    this._onDidChangeTreeData.dispose();
  }
}
