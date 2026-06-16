import * as path from "node:path";
// VS Code calls TreeDataProvider members through the registered provider.
// fallow-ignore-file unused-class-member
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { countCheckIssues } from "./analysis-utils.js";
import {
  middleElidePath,
  resolveFilePath as resolveFilePathPure,
  sortCloneGroupsBySize,
} from "./treeView-utils.js";
import { openFileCommand } from "./openFileCommand.js";
import type {
  CloneGroupFinding,
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
  "unused-store-member": "symbol-field",
  "unused-server-action": "symbol-method",
  "unused-load-data-keys": "symbol-property",
  "unused-component-prop": "symbol-property",
  "unused-component-emit": "symbol-event",
  "unused-component-input": "symbol-property",
  "unused-component-output": "symbol-event",
  "unrendered-component": "symbol-misc",
  "unprovided-inject": "plug",
  "invalid-client-export": "error",
  "mixed-client-server-barrel": "files",
  "misplaced-directive": "warning",
  "route-collision": "git-merge",
  "dynamic-segment-name-conflict": "git-merge",
  "unresolved-imports": "error",
  "unlisted-dependencies": "package",
  "duplicate-exports": "files",
  "type-only-dependencies": "symbol-interface",
  "test-only-dependencies": "beaker",
  "circular-dependencies": "sync",
  "re-export-cycles": "sync-ignored",
  "boundary-violation": "symbol-namespace",
  "policy-violations": "symbol-namespace",
  "stale-suppressions": "trash",
  "unused-catalog-entries": "package",
  "empty-catalog-groups": "package",
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
  "unused-store-member": "symbol-field",
  "unused-server-action": "symbol-method",
  "unused-load-data-keys": "symbol-property",
  "unused-component-prop": "symbol-property",
  "unused-component-emit": "symbol-event",
  "unused-component-input": "symbol-property",
  "unused-component-output": "symbol-event",
  "unrendered-component": "symbol-misc",
  "unprovided-inject": "plug",
  "invalid-client-export": "error",
  "mixed-client-server-barrel": "files",
  "misplaced-directive": "warning",
  "route-collision": "git-merge",
  "dynamic-segment-name-conflict": "git-merge",
  "unresolved-imports": "error",
  "unlisted-dependencies": "package",
  "duplicate-exports": "copy",
  "type-only-dependencies": "package",
  "test-only-dependencies": "beaker",
  "circular-dependencies": "sync",
  "re-export-cycles": "sync-ignored",
  "boundary-violation": "symbol-namespace",
  "policy-violations": "symbol-namespace",
  "stale-suppressions": "trash",
  "unused-catalog-entries": "package",
  "empty-catalog-groups": "package",
  "unresolved-catalog-references": "error",
  "unused-dependency-overrides": "package",
  "misconfigured-dependency-overrides": "error",
};

const staleSuppressionLabel = (
  origin: NonNullable<FallowCheckResult["stale_suppressions"]>[number]["origin"],
): string => {
  if (origin.type === "jsdoc_tag") {
    return `@expected-unused ${origin.export_name}`;
  }
  if (origin.issue_kind) {
    return origin.is_file_level ? `file ${origin.issue_kind}` : origin.issue_kind;
  }
  return origin.is_file_level ? "file suppression" : "line suppression";
};

type DeadCodeItem = CategoryItem | IssueItem | CycleItem;

class CategoryItem extends vscode.TreeItem {
  readonly issues: ReadonlyArray<IssueItem | CycleItem>;

  constructor(
    readonly category: IssueCategory,
    issues: ReadonlyArray<IssueItem | CycleItem>,
  ) {
    super(
      `${ISSUE_CATEGORY_LABELS[category]} (${issues.length})`,
      vscode.TreeItemCollapsibleState.Collapsed,
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
    category: IssueCategory,
  ) {
    super(label, vscode.TreeItemCollapsibleState.None);

    const { absolute, relative } = resolveFilePath(filePath);

    this.description = `${middleElidePath(relative)}:${line}`;
    this.tooltip = `${label}\n${absolute}:${line}:${col}`;
    this.contextValue = "issue";

    this.command = openFileCommand(absolute, line, col);

    this.iconPath = new vscode.ThemeIcon(ISSUE_ICONS[category] ?? "warning");
  }
}

/**
 * A dependency cycle (circular dependency or re-export cycle). Collapsible: the
 * label summarizes the cycle (`N files`) and the children are every file in the
 * cycle, each clickable, so the whole loop is visible rather than just the
 * entry file.
 */
class CycleItem extends vscode.TreeItem {
  readonly fileItems: ReadonlyArray<IssueItem>;

  constructor(label: string, files: ReadonlyArray<string>, category: IssueCategory) {
    super(
      label,
      files.length > 0
        ? vscode.TreeItemCollapsibleState.Collapsed
        : vscode.TreeItemCollapsibleState.None,
    );
    this.fileItems = files.map((f) => new IssueItem(path.basename(f), f, 1, 0, category));
    const { relative } = resolveFilePath(files[0] ?? "");
    this.description = relative ? middleElidePath(relative) : undefined;
    this.tooltip = files.map((f) => resolveFilePath(f).absolute).join("\n");
    this.contextValue = "cycle";
    this.iconPath = new vscode.ThemeIcon(ISSUE_ICONS[category] ?? "warning");
  }
}

export class DeadCodeTreeProvider implements vscode.TreeDataProvider<DeadCodeItem> {
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

    this.view.badge =
      count > 0 ? { value: count, tooltip: `${count} issue${count === 1 ? "" : "s"}` } : undefined;
  }

  getTreeItem(element: DeadCodeItem): vscode.TreeItem {
    return element;
  }

  getChildren(element?: DeadCodeItem): DeadCodeItem[] {
    if (element instanceof CategoryItem) {
      return [...element.issues];
    }

    if (element instanceof CycleItem) {
      return [...element.fileItems];
    }

    if (!this.result) {
      return [];
    }

    const categories: DeadCodeItem[] = [];

    const addCategory = (
      category: IssueCategory,
      items: ReadonlyArray<IssueItem | CycleItem>,
    ): void => {
      if (items.length > 0) {
        categories.push(new CategoryItem(category, items));
      }
    };

    addCategory(
      "unused-files",
      this.result.unused_files.map(
        (f) => new IssueItem(path.basename(f.path), f.path, 1, 0, "unused-files"),
      ),
    );

    addCategory(
      "unused-exports",
      this.result.unused_exports.map(
        (e) => new IssueItem(e.export_name, e.path, e.line, e.col, "unused-exports"),
      ),
    );

    addCategory(
      "unused-types",
      this.result.unused_types.map(
        (e) => new IssueItem(e.export_name, e.path, e.line, e.col, "unused-types"),
      ),
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
            "private-type-leaks",
          ),
      ),
    );

    addCategory(
      "unused-dependencies",
      this.result.unused_dependencies.map(
        (d) => new IssueItem(d.package_name, d.path, d.line, 0, "unused-dependencies"),
      ),
    );

    addCategory(
      "unused-dev-dependencies",
      this.result.unused_dev_dependencies.map(
        (d) => new IssueItem(d.package_name, d.path, d.line, 0, "unused-dev-dependencies"),
      ),
    );

    if (this.result.unused_optional_dependencies) {
      addCategory(
        "unused-optional-dependencies",
        this.result.unused_optional_dependencies.map(
          (d) => new IssueItem(d.package_name, d.path, d.line, 0, "unused-optional-dependencies"),
        ),
      );
    }

    addCategory(
      "unused-enum-members",
      this.result.unused_enum_members.map(
        (m) =>
          new IssueItem(
            `${m.parent_name}.${m.member_name}`,
            m.path,
            m.line,
            m.col,
            "unused-enum-members",
          ),
      ),
    );

    addCategory(
      "unused-class-members",
      this.result.unused_class_members.map(
        (m) =>
          new IssueItem(
            `${m.parent_name}.${m.member_name}`,
            m.path,
            m.line,
            m.col,
            "unused-class-members",
          ),
      ),
    );

    if (this.result.unused_store_members) {
      addCategory(
        "unused-store-member",
        this.result.unused_store_members.map(
          (m) =>
            new IssueItem(
              `${m.parent_name}.${m.member_name}`,
              m.path,
              m.line,
              m.col,
              "unused-store-member",
            ),
        ),
      );
    }

    if (this.result.unused_server_actions) {
      addCategory(
        "unused-server-action",
        this.result.unused_server_actions.map(
          (a) => new IssueItem(a.action_name, a.path, a.line, a.col, "unused-server-action"),
        ),
      );
    }

    if (this.result.unused_load_data_keys) {
      addCategory(
        "unused-load-data-keys",
        this.result.unused_load_data_keys.map(
          (k) => new IssueItem(k.key_name, k.path, k.line, k.col, "unused-load-data-keys"),
        ),
      );
    }

    if (this.result.unused_component_props) {
      addCategory(
        "unused-component-prop",
        this.result.unused_component_props.map(
          (p) =>
            new IssueItem(
              `${p.component_name}.${p.prop_name}`,
              p.path,
              p.line,
              p.col,
              "unused-component-prop",
            ),
        ),
      );
    }

    if (this.result.unused_component_emits) {
      addCategory(
        "unused-component-emit",
        this.result.unused_component_emits.map(
          (e) =>
            new IssueItem(
              `${e.component_name}.${e.emit_name}`,
              e.path,
              e.line,
              e.col,
              "unused-component-emit",
            ),
        ),
      );
    }

    if (this.result.unused_component_inputs) {
      addCategory(
        "unused-component-input",
        this.result.unused_component_inputs.map(
          (i) =>
            new IssueItem(
              `${i.component_name}.${i.input_name}`,
              i.path,
              i.line,
              i.col,
              "unused-component-input",
            ),
        ),
      );
    }

    if (this.result.unused_component_outputs) {
      addCategory(
        "unused-component-output",
        this.result.unused_component_outputs.map(
          (o) =>
            new IssueItem(
              `${o.component_name}.${o.output_name}`,
              o.path,
              o.line,
              o.col,
              "unused-component-output",
            ),
        ),
      );
    }

    if (this.result.unrendered_components) {
      addCategory(
        "unrendered-component",
        this.result.unrendered_components.map(
          (c) => new IssueItem(c.component_name, c.path, c.line, c.col, "unrendered-component"),
        ),
      );
    }

    if (this.result.unprovided_injects) {
      addCategory(
        "unprovided-inject",
        this.result.unprovided_injects.map(
          (i) => new IssueItem(i.key_name, i.path, i.line, i.col, "unprovided-inject"),
        ),
      );
    }

    if (this.result.invalid_client_exports) {
      addCategory(
        "invalid-client-export",
        this.result.invalid_client_exports.map(
          (e) => new IssueItem(e.export_name, e.path, e.line, e.col, "invalid-client-export"),
        ),
      );
    }

    if (this.result.mixed_client_server_barrels) {
      addCategory(
        "mixed-client-server-barrel",
        this.result.mixed_client_server_barrels.map(
          (b) =>
            new IssueItem(
              `${b.client_origin} + ${b.server_origin}`,
              b.path,
              b.line,
              b.col,
              "mixed-client-server-barrel",
            ),
        ),
      );
    }

    if (this.result.misplaced_directives) {
      addCategory(
        "misplaced-directive",
        this.result.misplaced_directives.map(
          (d) => new IssueItem(d.directive, d.path, d.line, d.col, "misplaced-directive"),
        ),
      );
    }

    if (this.result.route_collisions) {
      addCategory(
        "route-collision",
        this.result.route_collisions.map(
          (r) => new IssueItem(r.url, r.path, r.line, r.col, "route-collision"),
        ),
      );
    }

    if (this.result.dynamic_segment_name_conflicts) {
      addCategory(
        "dynamic-segment-name-conflict",
        this.result.dynamic_segment_name_conflicts.map(
          (c) =>
            new IssueItem(
              `${c.position} (${c.conflicting_segments.join(" vs ")})`,
              c.path,
              c.line,
              c.col,
              "dynamic-segment-name-conflict",
            ),
        ),
      );
    }

    addCategory(
      "unresolved-imports",
      this.result.unresolved_imports.map(
        (i) => new IssueItem(i.specifier, i.path, i.line, i.col, "unresolved-imports"),
      ),
    );

    addCategory(
      "unlisted-dependencies",
      this.result.unlisted_dependencies.flatMap((d) =>
        d.imported_from.map(
          (site) =>
            new IssueItem(d.package_name, site.path, site.line, site.col, "unlisted-dependencies"),
        ),
      ),
    );

    addCategory(
      "duplicate-exports",
      this.result.duplicate_exports.flatMap((d) =>
        d.locations.map(
          (loc) => new IssueItem(d.export_name, loc.path, loc.line, loc.col, "duplicate-exports"),
        ),
      ),
    );

    if (this.result.type_only_dependencies) {
      addCategory(
        "type-only-dependencies",
        this.result.type_only_dependencies.map(
          (d) => new IssueItem(d.package_name, d.path, d.line, 0, "type-only-dependencies"),
        ),
      );
    }

    if (this.result.test_only_dependencies) {
      addCategory(
        "test-only-dependencies",
        this.result.test_only_dependencies.map(
          (d) => new IssueItem(d.package_name, d.path, d.line, 0, "test-only-dependencies"),
        ),
      );
    }

    if (this.result.circular_dependencies) {
      addCategory(
        "circular-dependencies",
        this.result.circular_dependencies.map(
          (c) => new CycleItem(`${c.length} files`, c.files, "circular-dependencies"),
        ),
      );
    }

    if (this.result.re_export_cycles) {
      addCategory(
        "re-export-cycles",
        this.result.re_export_cycles.map(
          (c) =>
            new CycleItem(
              c.kind === "self-loop" ? "Self-loop" : `${c.files.length} files`,
              c.files,
              "re-export-cycles",
            ),
        ),
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
              "boundary-violation",
            ),
        ),
      );
    }

    if (this.result.policy_violations) {
      addCategory(
        "policy-violations",
        this.result.policy_violations.map(
          (v) =>
            new IssueItem(`${v.pack}/${v.rule_id}`, v.path, v.line, v.col, "policy-violations"),
        ),
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
              "stale-suppressions",
            ),
        ),
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
              "unused-catalog-entries",
            ),
        ),
      );
    }

    if (this.result.empty_catalog_groups) {
      addCategory(
        "empty-catalog-groups",
        this.result.empty_catalog_groups.map(
          (group) =>
            new IssueItem(group.catalog_name, group.path, group.line, 0, "empty-catalog-groups"),
        ),
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
              "unresolved-catalog-references",
            ),
        ),
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
              "unused-dependency-overrides",
            ),
        ),
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
              "misconfigured-dependency-overrides",
            ),
        ),
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

  constructor(group: CloneGroupFinding) {
    const instanceItems = group.instances.map(
      (inst) => new CloneInstanceItem(inst.file, inst.start_line, inst.end_line),
    );
    // Name the clone by what it is: fallow's dominant repeated identifier (e.g.
    // a shared `parseCsv` function), falling back to the first instance's file
    // basename when the clone has no clear name. The list is already ordered by
    // impact, so an opaque "Clone #N" ordinal is not needed.
    const name =
      group.suggested_name ??
      (group.instances[0] ? path.basename(group.instances[0].file) : "Duplicated code");
    const count = group.instances.length;
    super(name, vscode.TreeItemCollapsibleState.Collapsed);
    this.description = `${group.line_count} lines · ${count} instance${count === 1 ? "" : "s"}`;
    this.tooltip = `${name}\n${group.line_count} lines · ${count} instance${count === 1 ? "" : "s"} · ${group.fingerprint}`;
    this.instances = instanceItems;
    this.contextValue = "cloneFamily";
    this.iconPath = new vscode.ThemeIcon("files");
  }
}

class CloneInstanceItem extends vscode.TreeItem {
  constructor(
    readonly filePath: string,
    readonly startLine: number,
    readonly endLine: number,
  ) {
    const basename = path.basename(filePath);
    super(`${basename}:${startLine}-${endLine}`, vscode.TreeItemCollapsibleState.None);

    const { absolute, relative } = resolveFilePath(filePath);

    this.description = middleElidePath(relative);
    this.tooltip = `${absolute}:${startLine}-${endLine}`;
    this.contextValue = "cloneInstance";

    this.command = openFileCommand(absolute, startLine, 0, endLine, 0);

    this.iconPath = new vscode.ThemeIcon("copy");
  }
}

export class DuplicatesTreeProvider implements vscode.TreeDataProvider<DuplicateItem> {
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

    return sortCloneGroupsBySize(this.result.clone_groups).map(
      (group) => new CloneFamilyItem(group),
    );
  }

  dispose(): void {
    this._onDidChangeTreeData.dispose();
  }
}
