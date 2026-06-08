import * as vscode from "vscode";
import type { ComplexityDecorationController } from "./complexityDecorations.js";
import { getComplexityBreakdownEnabled, getHealthInlineComplexity } from "./config.js";

/** Command id wired in extension.ts; toggles one function's per-line breakdown. */
export const TOGGLE_COMPLEXITY_BREAKDOWN_COMMAND = "fallow.toggleComplexityBreakdown";

/** Payload the lens passes to the toggle command (a single complex function). */
export interface ComplexityToggleTarget {
  readonly path: string;
  readonly line: number;
}

/**
 * One code lens per complex function, anchored at its signature line, showing
 * the compact `N cyc, N cog` summary plus a `show breakdown` / `hide breakdown`
 * toggle that flips with the function's pin state. Clicking runs
 * `fallow.toggleComplexityBreakdown`, which pins/unpins the per-line detail in
 * `ComplexityDecorationController`.
 *
 * This is the VS Code-native replacement for the LSP complexity lens: the LSP
 * lens stays available to other editors (Neovim/Zed/Helix) but is not requested
 * by this extension, so there is no double lens here. Gated on
 * `fallow.health.inlineComplexity` (default on).
 */
export class ComplexityLensProvider implements vscode.CodeLensProvider {
  readonly onDidChangeCodeLenses: vscode.Event<void>;

  constructor(private readonly controller: ComplexityDecorationController) {
    // Re-emit lenses whenever the expansion state or findings change so titles
    // flip (`show` <-> `hide`) and lenses clear on stale documents in step.
    this.onDidChangeCodeLenses = controller.onDidChange;
  }

  provideCodeLenses(document: vscode.TextDocument): vscode.CodeLens[] {
    // `breakdownEnabled` is the master switch for the whole feature; the lens is
    // additionally gated on `inlineComplexity`.
    if (
      !getComplexityBreakdownEnabled() ||
      !getHealthInlineComplexity() ||
      document.uri.scheme !== "file"
    ) {
      return [];
    }
    if (this.controller.isStale(document)) {
      return [];
    }
    const lenses: vscode.CodeLens[] = [];
    for (const finding of this.controller.findingsForDocument(document.uri.fsPath)) {
      const line = Math.max(0, finding.line - 1);
      const range = new vscode.Range(line, 0, line, 0);
      const expanded = this.controller.isExpanded(finding.path, finding.line);
      const toggle = expanded ? "hide breakdown" : "show breakdown";
      const target: ComplexityToggleTarget = { path: finding.path, line: finding.line };
      lenses.push(
        new vscode.CodeLens(range, {
          title: `${finding.name}: ${finding.cyclomatic} cyc, ${finding.cognitive} cog · ${toggle}`,
          command: TOGGLE_COMPLEXITY_BREAKDOWN_COMMAND,
          arguments: [target],
        }),
      );
    }
    return lenses;
  }
}
