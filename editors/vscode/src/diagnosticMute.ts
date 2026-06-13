// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  DiagnosticFilter,
  diagnosticCode,
  getDiagnosticCategories,
  isFallowDiagnostic,
} from "./diagnosticFilter.js";

const DUPLICATE_CODE = "code-duplication";
const STATUS_ITEM_ID = "fallow.diagnosticMutes";
const CODE_ACTION_KIND = vscode.CodeActionKind.QuickFix.append("fallow.mute");
const FALLOW_LANGUAGES = [
  "javascript",
  "javascriptreact",
  "typescript",
  "typescriptreact",
  "vue",
  "svelte",
  "astro",
  "mdx",
  "json",
];

const labelFor = (code: string): string =>
  getDiagnosticCategories().find((c) => c.code === code)?.label ?? code;

const categoryWord = (count: number): string => (count === 1 ? "category" : "categories");

const muteScopeTooltip = (filter: DiagnosticFilter): vscode.MarkdownString => {
  const muted = Array.from(filter.mutedCategoriesSnapshot()).map(labelFor).toSorted();
  const mutedAll = filter.isMutedAll();
  const lines: string[] = [];
  if (mutedAll) {
    lines.push("**All Fallow findings hidden** in the editor.");
  } else if (muted.length > 0) {
    lines.push(`**Hiding ${muted.length} ${categoryWord(muted.length)}** in the editor:`);
    lines.push("");
    for (const m of muted) {
      lines.push(`- ${m}`);
    }
  } else {
    lines.push("All Fallow findings visible.");
  }
  lines.push("");
  lines.push("Local view filter only. CI and `fallow check` still report every finding.");
  lines.push("To disable a rule project-wide, edit your fallow config.");
  const md = new vscode.MarkdownString(lines.join("\n"));
  md.isTrusted = false;
  md.supportThemeIcons = true;
  return md;
};

const summaryText = (filter: DiagnosticFilter): string => {
  if (filter.isMutedAll()) {
    return "Fallow: hiding all";
  }
  const n = filter.mutedCategoriesSnapshot().size;
  return `Fallow: hiding ${n} ${categoryWord(n)}`;
};

/** A LanguageStatusItem in the right gutter that surfaces mute state.
 *  Severity is `Warning` whenever anything is muted, otherwise the item is
 *  hidden. Click opens the manage-mutes QuickPick. A secondary command
 *  clears all mutes in one click.
 *
 *  Returns a composite disposable that tears down both the status item and the
 *  `filter.onDidChange` subscription, so a re-create does not leak listeners
 *  (and disposal no longer relies on LIFO ordering of the status item). */
const createLanguageStatus = (filter: DiagnosticFilter): vscode.Disposable => {
  const selector = FALLOW_LANGUAGES.map((language) => ({
    scheme: "file",
    language,
  }));
  const item = vscode.languages.createLanguageStatusItem(STATUS_ITEM_ID, []);
  item.name = "Fallow Mute";
  item.accessibilityInformation = {
    label: "Fallow hidden findings status",
    role: "button",
  };

  const apply = (): void => {
    if (!filter.anythingMuted()) {
      item.selector = [];
      item.severity = vscode.LanguageStatusSeverity.Information;
      item.text = "$(check) Fallow";
      item.detail = "all findings visible";
      item.command = undefined;
      return;
    }
    item.selector = selector;
    item.severity = vscode.LanguageStatusSeverity.Warning;
    item.text = `$(eye-closed) ${summaryText(filter)}`;
    item.detail = "click to manage";
    item.command = {
      command: "fallow.manageDiagnosticMutes",
      title: "Manage",
      tooltip: "Manage Fallow hidden findings",
    };
  };

  apply();
  const subscription = filter.onDidChange(apply);
  return {
    dispose: () => {
      subscription.dispose();
      item.dispose();
    },
  };
};

interface ManagePickItem extends vscode.QuickPickItem {
  readonly code: string | null;
}

const TITLE_BUTTONS = {
  toggleAll: {
    iconPath: new vscode.ThemeIcon("eye-closed"),
    tooltip: "Hide or show all Fallow findings",
  },
  clearAll: {
    iconPath: new vscode.ThemeIcon("clear-all"),
    tooltip: "Show all Fallow findings (clear all)",
  },
} as const;

const showManageQuickPick = async (filter: DiagnosticFilter): Promise<void> => {
  const pick = vscode.window.createQuickPick<ManagePickItem>();
  pick.title = "Fallow: manage hidden findings (CI is unaffected)";
  pick.placeholder = "Check categories to hide them in the editor. Press Enter to apply.";
  pick.canSelectMany = true;
  pick.matchOnDetail = true;
  pick.buttons = [TITLE_BUTTONS.toggleAll, TITLE_BUTTONS.clearAll];

  const globalItem: ManagePickItem = {
    label: "$(eye-closed) All Fallow Findings",
    description: filter.isMutedAll() ? "currently hidden" : "currently visible",
    detail: "Hides all findings in the editor only. Use the title buttons to toggle or clear it.",
    code: null,
    picked: filter.isMutedAll(),
    alwaysShow: filter.isMutedAll(),
  };
  const items: ManagePickItem[] = [
    globalItem,
    ...getDiagnosticCategories().map(({ code, label }) => ({
      label,
      description: code,
      code,
      // Reflect the genuine per-category mute state, NOT `isMutedAll()`. When
      // hide-all is on, auto-checking every category made unchecking the global
      // "All Findings" row re-mute each category individually on accept (the
      // `else` branch applies `setMutedCategories(selected)`), so the user who
      // unchecked it to reveal findings stayed fully hidden. mute-all is a
      // separate flag the filter tracks independently, so per-category rows show
      // their own state and unchecking the global row reveals what is actually
      // hidden underneath.
      picked: filter.isCategoryMuted(code),
    })),
  ];
  pick.items = items;
  pick.selectedItems = items.filter((i) => i.picked === true);

  await new Promise<void>((resolve) => {
    pick.onDidTriggerButton((button) => {
      if (button === TITLE_BUTTONS.toggleAll) {
        filter.toggleMutedAll();
      } else if (button === TITLE_BUTTONS.clearAll) {
        filter.clearAllMutes();
      }
      pick.hide();
    });
    pick.onDidAccept(() => {
      const globalSelected = pick.selectedItems.some((i) => i.code === null);
      const selected = new Set(
        pick.selectedItems.map((i) => i.code).filter((code): code is string => code !== null),
      );
      if (globalSelected) {
        filter.setMutedAll(true);
      } else {
        // Turn off mute-all AND apply the category selection in one cycle, so a
        // single accept does not fire two persisted writes and two LSP re-pulls.
        filter.applyMuteSelection(false, selected);
      }
      pick.hide();
    });
    pick.onDidHide(() => {
      pick.dispose();
      resolve();
    });
    pick.show();
  });
};

const updateContextKey = (filter: DiagnosticFilter): void => {
  void vscode.commands.executeCommand(
    "setContext",
    "fallow.duplicatesMuted",
    filter.isCategoryMuted(DUPLICATE_CODE) || filter.isMutedAll(),
  );
  void vscode.commands.executeCommand(
    "setContext",
    "fallow.allDiagnosticsMuted",
    filter.isMutedAll(),
  );
};

class FallowMuteCodeActions implements vscode.CodeActionProvider {
  public static readonly providedKinds: ReadonlyArray<vscode.CodeActionKind> = [CODE_ACTION_KIND];

  public provideCodeActions(
    _document: vscode.TextDocument,
    _range: vscode.Range | vscode.Selection,
    context: vscode.CodeActionContext,
  ): vscode.CodeAction[] {
    const seen = new Set<string>();
    const actions: vscode.CodeAction[] = [];
    for (const diag of context.diagnostics) {
      if (!isFallowDiagnostic(diag)) {
        continue;
      }
      const code = diagnosticCode(diag);
      if (!code || seen.has(code)) {
        continue;
      }
      seen.add(code);
      const label = labelFor(code);
      const action = new vscode.CodeAction(
        `Hide Fallow ${label.toLowerCase()} findings in this workspace`,
        CODE_ACTION_KIND,
      );
      action.command = {
        command: "fallow.muteDiagnosticCategory",
        title: "Hide Fallow category",
        arguments: [code],
      };
      action.diagnostics = [diag];
      actions.push(action);
    }
    return actions;
  }
}

export const registerDiagnosticMuteUi = (
  context: vscode.ExtensionContext,
  filter: DiagnosticFilter,
): void => {
  context.subscriptions.push(createLanguageStatus(filter));

  context.subscriptions.push(filter.onDidChange(() => updateContextKey(filter)));
  updateContextKey(filter);

  context.subscriptions.push(
    vscode.workspace.onDidCloseTextDocument((doc) => {
      filter.evictUri(doc.uri);
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.toggleMuteDuplicates", () => {
      const nowMuted = filter.toggleCategory(DUPLICATE_CODE);
      void vscode.window.setStatusBarMessage(
        nowMuted
          ? "Fallow: hiding code-duplication findings (CI is unaffected)"
          : "Fallow: showing code-duplication findings",
        4000,
      );
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.toggleAllDiagnostics", () => {
      const nowMuted = filter.toggleMutedAll();
      void vscode.window.setStatusBarMessage(
        nowMuted
          ? "Fallow: hiding all findings (CI is unaffected)"
          : "Fallow: showing all findings",
        4000,
      );
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.manageDiagnosticMutes", async () => {
      await showManageQuickPick(filter);
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.clearDiagnosticMutes", () => {
      filter.clearAllMutes();
    }),
  );

  context.subscriptions.push(
    vscode.commands.registerCommand("fallow.muteDiagnosticCategory", (code: unknown) => {
      if (typeof code === "string" && code.length > 0) {
        filter.setCategoryMuted(code, true);
      }
    }),
  );

  for (const language of FALLOW_LANGUAGES) {
    context.subscriptions.push(
      vscode.languages.registerCodeActionsProvider(
        { scheme: "file", language },
        new FallowMuteCodeActions(),
        { providedCodeActionKinds: FallowMuteCodeActions.providedKinds },
      ),
    );
  }
};

export const __testHelpers = {
  createLanguageStatus,
  labelFor,
  summaryText,
  showManageQuickPick,
  muteScopeTooltip,
};
