import { afterEach, describe, expect, it, vi } from "vitest";

interface FakeStatusBarItem {
  text: string;
  tooltip: unknown;
  command: string | undefined;
  show: ReturnType<typeof vi.fn>;
  dispose: ReturnType<typeof vi.fn>;
}

const created: FakeStatusBarItem[] = [];

vi.mock("vscode", () => {
  const makeItem = (): FakeStatusBarItem => {
    const item: FakeStatusBarItem = {
      text: "",
      tooltip: undefined,
      command: undefined,
      show: vi.fn(),
      dispose: vi.fn(),
    };
    created.push(item);
    return item;
  };
  return {
    StatusBarAlignment: { Left: 1, Right: 2 },
    MarkdownString: class {
      public constructor(public readonly value: string) {}
    },
    window: {
      createStatusBarItem: vi.fn(() => makeItem()),
    },
  };
});

import type { DiagnosticFilter } from "../src/diagnosticFilter.js";
import {
  createDiagnosticStatusBar,
  diagnosticTogglePresentation,
  disposeDiagnosticStatusBar,
  hasDiagnosticStatusBar,
} from "../src/diagnosticStatusBar.js";

/** Minimal stand-in for DiagnosticFilter exposing only what the status bar reads. */
const makeFilter = () => {
  let mutedAll = false;
  let categories = new Set<string>();
  const listeners = new Set<() => void>();
  const filter = {
    isMutedAll: () => mutedAll,
    mutedCategoriesSnapshot: () => new Set(categories),
    onDidChange: (listener: () => void) => {
      listeners.add(listener);
      return { dispose: () => listeners.delete(listener) };
    },
  };
  const set = (next: { mutedAll?: boolean; categories?: readonly string[] }): void => {
    if (next.mutedAll !== undefined) {
      mutedAll = next.mutedAll;
    }
    if (next.categories !== undefined) {
      categories = new Set(next.categories);
    }
    for (const listener of listeners) {
      listener();
    }
  };
  return { filter: filter as unknown as DiagnosticFilter, set, listenerCount: () => listeners.size };
};

const tooltipValue = (item: FakeStatusBarItem): string => (item.tooltip as { value: string }).value;

afterEach(() => {
  disposeDiagnosticStatusBar();
  created.length = 0;
  vi.clearAllMocks();
});

describe("diagnosticTogglePresentation", () => {
  it("shows the hidden state when all findings are muted", () => {
    const p = diagnosticTogglePresentation(true, 0);
    expect(p.text).toBe("$(eye-closed) Fallow: hidden");
    expect(p.tooltip).toContain("hidden in this editor");
    expect(p.tooltip).toContain("Click to show");
    expect(p.tooltip).toContain("still report every finding");
    // Never the bare word "off" (reads as "extension/CI disabled").
    expect(p.text.toLowerCase()).not.toContain("off");
  });

  it("shows the visible state with a hide-all hint when nothing is muted", () => {
    const p = diagnosticTogglePresentation(false, 0);
    expect(p.text).toBe("$(eye) Fallow");
    expect(p.tooltip).toContain("Fallow findings are visible");
    expect(p.tooltip).toContain("Click to hide all");
    expect(p.tooltip).toContain("CI and `fallow check` are unaffected");
  });

  it("names the hidden-category count with singular grammar", () => {
    const p = diagnosticTogglePresentation(false, 1);
    expect(p.text).toBe("$(eye) Fallow");
    expect(p.tooltip).toContain("1 category hidden");
    expect(p.tooltip).not.toContain("1 categories");
  });

  it("names the hidden-category count with plural grammar", () => {
    const p = diagnosticTogglePresentation(false, 3);
    expect(p.tooltip).toContain("3 categories hidden");
    // The per-category set survives a hide-all / show round trip.
    expect(p.tooltip).toContain("kept when you show again");
  });
});

describe("diagnostic status bar lifecycle", () => {
  it("creates a visible item wired to the toggle command", () => {
    const { filter } = makeFilter();
    const item = createDiagnosticStatusBar(filter) as unknown as FakeStatusBarItem;

    expect(hasDiagnosticStatusBar()).toBe(true);
    expect(item.command).toBe("fallow.toggleAllDiagnostics");
    expect(item.text).toBe("$(eye) Fallow");
    expect(item.show).toHaveBeenCalledTimes(1);
  });

  it("re-renders when the filter state changes", () => {
    const { filter, set } = makeFilter();
    const item = createDiagnosticStatusBar(filter) as unknown as FakeStatusBarItem;
    expect(item.text).toBe("$(eye) Fallow");

    set({ mutedAll: true });
    expect(item.text).toBe("$(eye-closed) Fallow: hidden");

    set({ mutedAll: false, categories: ["code-duplication", "unused-export"] });
    expect(item.text).toBe("$(eye) Fallow");
    expect(tooltipValue(item)).toContain("2 categories hidden");
  });

  it("is idempotent: a second create disposes the first item and its subscription", () => {
    const { filter, listenerCount } = makeFilter();
    const first = createDiagnosticStatusBar(filter) as unknown as FakeStatusBarItem;
    const second = createDiagnosticStatusBar(filter) as unknown as FakeStatusBarItem;

    expect(first).not.toBe(second);
    expect(first.dispose).toHaveBeenCalledTimes(1);
    expect(hasDiagnosticStatusBar()).toBe(true);
    // Exactly one live subscription, not two (no leak from the first create).
    expect(listenerCount()).toBe(1);
  });

  it("disposes the item and unsubscribes from the filter", () => {
    const { filter, set, listenerCount } = makeFilter();
    const item = createDiagnosticStatusBar(filter) as unknown as FakeStatusBarItem;
    expect(listenerCount()).toBe(1);

    disposeDiagnosticStatusBar();
    expect(hasDiagnosticStatusBar()).toBe(false);
    expect(item.dispose).toHaveBeenCalledTimes(1);
    expect(listenerCount()).toBe(0);

    // A late filter event after dispose must not touch the disposed item.
    set({ mutedAll: true });
    expect(item.text).toBe("$(eye) Fallow");
  });
});
