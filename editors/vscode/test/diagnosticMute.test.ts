import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const vscodeMocks = vi.hoisted(() => ({
  createQuickPick: vi.fn(),
  createLanguageStatusItem: vi.fn((id: string, selector: unknown) => ({
    id,
    selector,
    name: undefined,
    severity: undefined,
    text: "",
    detail: undefined,
    command: undefined,
    dispose: vi.fn(),
  })),
}));

vi.mock("vscode", () => {
  type Listener<T> = (value: T) => void;
  class FakeEventEmitter<T> {
    private readonly listeners = new Set<Listener<T>>();
    public readonly event = (
      listener: Listener<T>
    ): { dispose: () => void } => {
      this.listeners.add(listener);
      return { dispose: () => this.listeners.delete(listener) };
    };
    public fire(value: T): void {
      for (const listener of this.listeners) {
        listener(value);
      }
    }
    public dispose(): void {
      this.listeners.clear();
    }
  }

  return {
    CodeActionKind: {
      QuickFix: {
        append: (value: string) => `quickfix.${value}`,
      },
    },
    EventEmitter: FakeEventEmitter,
    LanguageStatusSeverity: {
      Information: 0,
      Warning: 1,
    },
    MarkdownString: class {
      public isTrusted = false;
      public supportThemeIcons = false;
      public constructor(public readonly value: string) {}
    },
    ThemeIcon: class {
      public constructor(public readonly id: string) {}
    },
    Uri: {
      parse: (s: string) => ({ toString: () => s, scheme: "file" }),
    },
    languages: {
      createLanguageStatusItem: vscodeMocks.createLanguageStatusItem,
    },
    window: {
      createQuickPick: vscodeMocks.createQuickPick,
      setStatusBarMessage: vi.fn(),
    },
    workspace: {
      onDidCloseTextDocument: vi.fn(),
    },
    commands: {
      executeCommand: vi.fn(),
      registerCommand: vi.fn(),
    },
    CodeAction: class {
      public command: unknown;
      public diagnostics: unknown;
      public constructor(
        public readonly title: string,
        public readonly kind: string
      ) {}
    },
  };
});

import {
  DiagnosticFilter,
  resetDiagnosticCategories,
  setDiagnosticCategories,
} from "../src/diagnosticFilter.js";
import { __testHelpers } from "../src/diagnosticMute.js";

const memento = () => ({
  get: <T>(): T | undefined => undefined,
  update: vi.fn(async () => {}),
  keys: () => [],
});

const quickPickThatAcceptsDefaults = () => {
  let accept: (() => void) | undefined;
  let hide: (() => void) | undefined;
  return {
    title: "",
    placeholder: "",
    canSelectMany: false,
    matchOnDetail: false,
    buttons: [],
    items: [],
    selectedItems: [],
    onDidTriggerButton: vi.fn(() => ({ dispose: vi.fn() })),
    onDidAccept: vi.fn((listener: () => void) => {
      accept = listener;
      return { dispose: vi.fn() };
    }),
    onDidHide: vi.fn((listener: () => void) => {
      hide = listener;
      return { dispose: vi.fn() };
    }),
    show: vi.fn(() => {
      accept?.();
    }),
    hide: vi.fn(() => {
      hide?.();
    }),
    dispose: vi.fn(),
  };
};

beforeEach(() => {
  vscodeMocks.createLanguageStatusItem.mockClear();
});

afterEach(() => {
  resetDiagnosticCategories();
});

describe("diagnostic mute language status", () => {
  it("is hidden until a mute is active, then hides again after clearing", () => {
    const filter = new DiagnosticFilter(memento() as never);
    __testHelpers.createLanguageStatus(filter);
    const item = vscodeMocks.createLanguageStatusItem.mock.results.at(-1)
      ?.value as {
      selector: unknown;
      command: unknown;
    };

    expect(item.selector).toEqual([]);
    expect(item.command).toBeUndefined();

    filter.setCategoryMuted("code-duplication", true);
    expect(item.selector).toEqual([
      { scheme: "file", language: "javascript" },
      { scheme: "file", language: "javascriptreact" },
      { scheme: "file", language: "typescript" },
      { scheme: "file", language: "typescriptreact" },
      { scheme: "file", language: "vue" },
      { scheme: "file", language: "svelte" },
      { scheme: "file", language: "astro" },
      { scheme: "file", language: "mdx" },
      { scheme: "file", language: "json" },
    ]);
    expect(item.command).toMatchObject({
      command: "fallow.manageDiagnosticMutes",
    });

    filter.clearAllMutes();
    expect(item.selector).toEqual([]);
    expect(item.command).toBeUndefined();
  });

  it("disposing the status disposes both the item and the onDidChange subscription", () => {
    const filter = new DiagnosticFilter(memento() as never);
    const disposable = __testHelpers.createLanguageStatus(filter);
    const item = vscodeMocks.createLanguageStatusItem.mock.results.at(-1)
      ?.value as { dispose: ReturnType<typeof vi.fn> };

    // One listener is registered while the status is live; firing it updates the
    // item (it is not disposed yet).
    filter.setCategoryMuted("code-duplication", true);
    expect(item.dispose).not.toHaveBeenCalled();

    disposable.dispose();
    expect(item.dispose).toHaveBeenCalledTimes(1);

    // After disposal the subscription is gone: a state change must not throw or
    // re-touch the disposed item (no leaked listener across re-creates).
    const callsBefore = vscodeMocks.createLanguageStatusItem.mock.calls.length;
    expect(() => filter.clearAllMutes()).not.toThrow();
    expect(vscodeMocks.createLanguageStatusItem.mock.calls.length).toBe(
      callsBefore
    );
  });

  it("keeps global mute separate when accepting the default manage picker state", async () => {
    const pick = quickPickThatAcceptsDefaults();
    vscodeMocks.createQuickPick.mockReturnValueOnce(pick);
    const filter = new DiagnosticFilter(memento() as never);
    filter.setMutedAll(true);

    await __testHelpers.showManageQuickPick(filter);

    expect(filter.isMutedAll()).toBe(true);
    expect(filter.mutedCategoriesSnapshot().size).toBe(0);
    expect(
      (pick.selectedItems as Array<{ code: string | null }>).some(
        (item) => item.code === null
      )
    ).toBe(true);
  });

  it("unchecking 'All findings' in the manage picker reveals everything instead of re-muting every category", async () => {
    // Simulate: hide-all is on, the user opens Manage, unchecks the global
    // "All Fallow Findings" row, and accepts. They expect findings to come
    // back, not to stay hidden as per-category mutes.
    let acceptListener: (() => void) | undefined;
    let hideListener: (() => void) | undefined;
    const pick = {
      title: "",
      placeholder: "",
      canSelectMany: false,
      matchOnDetail: false,
      buttons: [] as unknown[],
      items: [] as Array<{ code: string | null }>,
      selectedItems: [] as Array<{ code: string | null }>,
      onDidTriggerButton: vi.fn(() => ({ dispose: vi.fn() })),
      onDidAccept: vi.fn((listener: () => void) => {
        acceptListener = listener;
        return { dispose: vi.fn() };
      }),
      onDidHide: vi.fn((listener: () => void) => {
        hideListener = listener;
        return { dispose: vi.fn() };
      }),
      show: vi.fn(() => {
        // User unchecks the global row (drops the null-code item) before accept.
        pick.selectedItems = pick.selectedItems.filter((item) => item.code !== null);
        acceptListener?.();
      }),
      hide: vi.fn(() => {
        hideListener?.();
      }),
      dispose: vi.fn(),
    };
    vscodeMocks.createQuickPick.mockReturnValueOnce(pick);
    const filter = new DiagnosticFilter(memento() as never);
    filter.setMutedAll(true);

    await __testHelpers.showManageQuickPick(filter);

    expect(filter.isMutedAll()).toBe(false);
    // Nothing should remain hidden: unchecking "All findings" must not silently
    // convert the hide-all into a hide-every-category.
    expect(filter.anythingMuted()).toBe(false);
  });

  it("unchecking 'All findings' preserves a genuine per-category mute", async () => {
    // hide-all on AND code-duplication individually hidden. The user unchecks
    // only the global row: hide-all turns off, the per-category mute survives.
    let acceptListener: (() => void) | undefined;
    let hideListener: (() => void) | undefined;
    const pick = {
      title: "",
      placeholder: "",
      canSelectMany: false,
      matchOnDetail: false,
      buttons: [] as unknown[],
      items: [] as Array<{ code: string | null }>,
      selectedItems: [] as Array<{ code: string | null }>,
      onDidTriggerButton: vi.fn(() => ({ dispose: vi.fn() })),
      onDidAccept: vi.fn((listener: () => void) => {
        acceptListener = listener;
        return { dispose: vi.fn() };
      }),
      onDidHide: vi.fn((listener: () => void) => {
        hideListener = listener;
        return { dispose: vi.fn() };
      }),
      show: vi.fn(() => {
        // Drop only the global row; keep the genuinely-muted category checked.
        pick.selectedItems = pick.selectedItems.filter((item) => item.code !== null);
        acceptListener?.();
      }),
      hide: vi.fn(() => {
        hideListener?.();
      }),
      dispose: vi.fn(),
    };
    vscodeMocks.createQuickPick.mockReturnValueOnce(pick);
    const filter = new DiagnosticFilter(memento() as never);
    filter.setCategoryMuted("code-duplication", true);
    filter.setMutedAll(true);

    await __testHelpers.showManageQuickPick(filter);

    expect(filter.isMutedAll()).toBe(false);
    expect(filter.isCategoryMuted("code-duplication")).toBe(true);
    expect(filter.isCategoryMuted("unused-export")).toBe(false);
  });

  it("uses LSP-provided categories for labels and the manage picker", async () => {
    setDiagnosticCategories([
      { code: "future-rule", label: "Future Rule" },
      { code: "unused-export", label: "Unused Exports" },
    ]);
    const pick = quickPickThatAcceptsDefaults();
    vscodeMocks.createQuickPick.mockReturnValueOnce(pick);
    const filter = new DiagnosticFilter(memento() as never);

    expect(__testHelpers.labelFor("future-rule")).toBe("Future Rule");

    await __testHelpers.showManageQuickPick(filter);

    expect(
      (pick.items as Array<{ code: string | null; label: string }>).map(
        (item) => [item.code, item.label]
      )
    ).toEqual([
      [null, "$(eye-closed) All Fallow Findings"],
      ["future-rule", "Future Rule"],
      ["unused-export", "Unused Exports"],
    ]);
  });
});
