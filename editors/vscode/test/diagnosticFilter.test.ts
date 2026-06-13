import { afterEach, describe, expect, it, vi } from "vitest";

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
      for (const l of this.listeners) {
        l(value);
      }
    }
    public dispose(): void {
      this.listeners.clear();
    }
  }
  return {
    DiagnosticSeverity: {
      Error: 0,
      Warning: 1,
      Information: 2,
      Hint: 3,
    },
    EventEmitter: FakeEventEmitter,
    Uri: {
      parse: (s: string) => ({ toString: () => s, scheme: "file" }),
    },
  };
});

import {
  DIAGNOSTIC_CATEGORIES,
  DiagnosticFilter,
  diagnosticCode,
  getDiagnosticCategories,
  isFallowDiagnostic,
  parseDiagnosticCategories,
  resetDiagnosticCategories,
  severityToDiagnosticSeverity,
  setDiagnosticCategories,
} from "../src/diagnosticFilter.js";

interface FakeDiag {
  source?: string;
  code?: string | number | { value: string | number };
  message: string;
  severity: number;
}

const diag = (overrides: Partial<FakeDiag>): FakeDiag => ({
  source: "fallow",
  message: "test",
  severity: 1,
  ...overrides,
});

const memento = (initial?: unknown) => {
  const store = new Map<string, unknown>();
  if (initial !== undefined) {
    store.set("fallow.diagnosticFilter.v1", initial);
  }
  return {
    get: <T>(key: string): T | undefined => store.get(key) as T | undefined,
    update: vi.fn(async (key: string, value: unknown) => {
      store.set(key, value);
    }),
    keys: () => Array.from(store.keys()),
    store,
  };
};

const fakeUri = (s: string) => ({ toString: () => s, scheme: "file" });

const flushPersistence = async (): Promise<void> => {
  await Promise.resolve();
  await Promise.resolve();
};

afterEach(() => {
  resetDiagnosticCategories();
});

const collection = () => {
  const sets: Array<{ uri: string; diags: FakeDiag[] }> = [];
  return {
    sets,
    set: (uri: { toString: () => string }, diags: FakeDiag[]) => {
      sets.push({ uri: uri.toString(), diags: diags.slice() });
    },
  };
};

describe("diagnosticCode", () => {
  it("extracts string codes", () => {
    expect(diagnosticCode(diag({ code: "code-duplication" }) as never)).toBe(
      "code-duplication"
    );
  });

  it("extracts numeric codes as strings", () => {
    expect(diagnosticCode(diag({ code: 42 }) as never)).toBe("42");
  });

  it("extracts object codes via .value", () => {
    expect(
      diagnosticCode(diag({ code: { value: "unused-export" } }) as never)
    ).toBe("unused-export");
    expect(diagnosticCode(diag({ code: { value: 7 } }) as never)).toBe("7");
  });

  it("returns null when code is absent", () => {
    expect(diagnosticCode(diag({}) as never)).toBeNull();
  });
});

describe("isFallowDiagnostic", () => {
  it("returns true only for source === fallow", () => {
    expect(isFallowDiagnostic(diag({ source: "fallow" }) as never)).toBe(true);
    expect(isFallowDiagnostic(diag({ source: "ts" }) as never)).toBe(false);
    expect(isFallowDiagnostic(diag({ source: undefined }) as never)).toBe(
      false
    );
  });
});

describe("DiagnosticFilter.applyFilter", () => {
  it("maps diagnostic severity settings to VS Code severities", () => {
    expect(severityToDiagnosticSeverity("warning")).toBe(1);
    expect(severityToDiagnosticSeverity("information")).toBe(2);
    expect(severityToDiagnosticSeverity("hint")).toBe(3);
  });

  it("passes everything through when nothing is muted", () => {
    const f = new DiagnosticFilter(memento() as never);
    const input = [
      diag({ code: "code-duplication" }),
      diag({ code: "unused-export" }),
      diag({ source: "ts", code: "2304" }),
    ];
    expect(f.applyFilter(input as never)).toEqual(input);
  });

  it("renders only fallow diagnostics as information", () => {
    const f = new DiagnosticFilter(memento() as never, () => "information");
    const input = [
      diag({ code: "code-duplication" }),
      diag({ source: "ts", code: "2304", severity: 1 }),
    ];
    const out = f.applyFilter(input as never);
    expect(out[0]?.severity).toBe(2);
    expect(out[1]?.severity).toBe(1);
  });

  it("renders fallow diagnostics as hints after mute filtering", () => {
    const f = new DiagnosticFilter(memento() as never, () => "hint");
    f.setCategoryMuted("code-duplication", true);
    const input = [
      diag({ code: "code-duplication" }),
      diag({ code: "unused-export" }),
    ];
    const out = f.applyFilter(input as never);
    expect(out).toHaveLength(1);
    expect(out[0]?.code).toBe("unused-export");
    expect(out[0]?.severity).toBe(3);
  });

  it("drops only fallow diagnostics with the muted code", () => {
    const f = new DiagnosticFilter(memento() as never);
    f.setCategoryMuted("code-duplication", true);
    const input = [
      diag({ code: "code-duplication" }),
      diag({ code: "unused-export" }),
      diag({ source: "eslint", code: "code-duplication" }),
    ];
    const out = f.applyFilter(input as never);
    expect(out).toHaveLength(2);
    expect(out.map((d) => d.code)).toEqual(["unused-export", "code-duplication"]);
    expect(out[1]?.source).toBe("eslint");
  });

  it("applies a team baseline on first open", () => {
    const f = new DiagnosticFilter(
      memento() as never,
      () => "warning",
      new Set(["code-duplication"])
    );
    const out = f.applyFilter(
      [
        diag({ code: "code-duplication" }),
        diag({ code: "unused-export" }),
      ] as never
    );

    expect(out.map((d) => d.code)).toEqual(["unused-export"]);
  });

  it("lets local mutes hide more than the team baseline", () => {
    const f = new DiagnosticFilter(
      memento() as never,
      () => "warning",
      new Set(["code-duplication"])
    );
    f.setCategoryMuted("unused-export", true);

    const out = f.applyFilter(
      [
        diag({ code: "code-duplication" }),
        diag({ code: "unused-export" }),
        diag({ code: "stale-suppression" }),
      ] as never
    );

    expect(out.map((d) => d.code)).toEqual(["stale-suppression"]);
  });

  it("lets a local override show a baseline-hidden category", () => {
    const f = new DiagnosticFilter(
      memento() as never,
      () => "warning",
      new Set(["code-duplication"])
    );
    f.setCategoryMuted("code-duplication", false);

    const out = f.applyFilter(
      [
        diag({ code: "code-duplication" }),
        diag({ code: "unused-export" }),
      ] as never
    );

    expect(out.map((d) => d.code)).toEqual(["code-duplication", "unused-export"]);
  });

  it("drops every fallow diagnostic when mutedAll is set, but never others", () => {
    const f = new DiagnosticFilter(memento() as never);
    f.setMutedAll(true);
    const input = [
      diag({ code: "code-duplication" }),
      diag({ code: "unused-export" }),
      diag({ source: "ts", code: "2304" }),
    ];
    const out = f.applyFilter(input as never);
    expect(out).toHaveLength(1);
    expect(out[0]?.source).toBe("ts");
  });

  it("keeps fallow diagnostics whose code is absent or unrecognized", () => {
    const f = new DiagnosticFilter(memento() as never);
    f.setCategoryMuted("code-duplication", true);
    const input = [
      diag({ code: undefined }),
      diag({ code: "novel-future-code" }),
    ];
    expect(f.applyFilter(input as never)).toHaveLength(2);
  });
});

describe("DiagnosticFilter persistence", () => {
  it("hydrates from memento on construction", () => {
    const m = memento({
      mutedAll: false,
      mutedCategories: ["code-duplication", "unused-export"],
    });
    const f = new DiagnosticFilter(m as never);
    expect(f.isCategoryMuted("code-duplication")).toBe(true);
    expect(f.isCategoryMuted("unused-export")).toBe(true);
    expect(f.isMutedAll()).toBe(false);
  });

  it("treats legacy muted categories as local mutes", () => {
    const m = memento({
      mutedAll: false,
      mutedCategories: ["unused-export"],
    });
    const f = new DiagnosticFilter(
      m as never,
      () => "warning",
      new Set(["code-duplication"])
    );

    expect(f.isCategoryMuted("code-duplication")).toBe(true);
    expect(f.isCategoryMuted("unused-export")).toBe(true);
  });

  it("writes through to memento on every change", async () => {
    const m = memento();
    const f = new DiagnosticFilter(m as never);
    f.setCategoryMuted("code-duplication", true);
    await flushPersistence();
    expect(m.update).toHaveBeenCalledWith(
      "fallow.diagnosticFilter.v1",
      expect.objectContaining({ mutedCategories: ["code-duplication"] })
    );
    f.setMutedAll(true);
    await flushPersistence();
    expect(m.update).toHaveBeenLastCalledWith(
      "fallow.diagnosticFilter.v1",
      expect.objectContaining({ mutedAll: true })
    );
  });

  it("persists local visibility overrides for baseline categories", async () => {
    const m = memento();
    const f = new DiagnosticFilter(
      m as never,
      () => "warning",
      new Set(["code-duplication"])
    );

    f.setCategoryMuted("code-duplication", false);
    await flushPersistence();

    expect(m.update).toHaveBeenLastCalledWith(
      "fallow.diagnosticFilter.v1",
      expect.objectContaining({
        localVisibleCategories: ["code-duplication"],
        mutedCategories: [],
      })
    );
  });

  it("clear all keeps baseline categories visible locally", async () => {
    const m = memento();
    const f = new DiagnosticFilter(
      m as never,
      () => "warning",
      new Set(["code-duplication"])
    );

    f.clearAllMutes();
    await flushPersistence();

    expect(f.isCategoryMuted("code-duplication")).toBe(false);
    expect(m.update).toHaveBeenLastCalledWith(
      "fallow.diagnosticFilter.v1",
      expect.objectContaining({
        localVisibleCategories: ["code-duplication"],
        mutedCategories: [],
      })
    );
  });

  it("updates a category set with one persisted write", async () => {
    const m = memento();
    const f = new DiagnosticFilter(m as never);
    f.setMutedCategories(new Set(["code-duplication", "unused-export"]));
    await flushPersistence();
    expect(m.update).toHaveBeenCalledTimes(1);
    expect(m.update).toHaveBeenLastCalledWith(
      "fallow.diagnosticFilter.v1",
      expect.objectContaining({
        mutedCategories: ["code-duplication", "unused-export"],
      })
    );
  });
});

describe("DiagnosticFilter corrupt-state recovery", () => {
  it("does not throw when the persisted value is not an object", () => {
    // A downgrade or hand-edit can leave a non-object (here a number) under the
    // state key; the constructor must recover silently rather than disabling the
    // whole extension for that workspace.
    expect(() => new DiagnosticFilter(memento(42) as never)).not.toThrow();
    const f = new DiagnosticFilter(memento(42) as never);
    expect(f.isMutedAll()).toBe(false);
    expect(f.anythingMuted()).toBe(false);
  });

  it("recovers to nothing-muted when array fields hold the wrong type", () => {
    const f = new DiagnosticFilter(
      memento({
        mutedAll: "yes",
        mutedCategories: 7,
        localVisibleCategories: { a: 1 },
      }) as never
    );
    expect(f.isMutedAll()).toBe(false);
    expect(f.anythingMuted()).toBe(false);
  });

  it("filters non-string members out of a persisted category array", () => {
    const f = new DiagnosticFilter(
      memento({
        mutedAll: false,
        mutedCategories: ["code-duplication", 5, null, "unused-export"],
      }) as never
    );
    expect(f.isCategoryMuted("code-duplication")).toBe(true);
    expect(f.isCategoryMuted("unused-export")).toBe(true);
  });
});

describe("DiagnosticFilter.applyMuteSelection", () => {
  it("turns mute-all off and applies the selection in one persisted write", async () => {
    const m = memento();
    const f = new DiagnosticFilter(m as never, () => "warning", new Set(["code-duplication"]));

    f.setMutedAll(true);
    await flushPersistence();
    m.update.mockClear();

    // Mirrors the manage pick's accept when the global "All Findings" row is
    // unchecked: select only `unused-export`, revealing the baseline category.
    f.applyMuteSelection(false, new Set(["unused-export"]));
    await flushPersistence();

    expect(f.isMutedAll()).toBe(false);
    expect(f.isCategoryMuted("unused-export")).toBe(true);
    expect(f.isCategoryMuted("code-duplication")).toBe(false);
    // One accept => exactly one persisted write (the old setMutedAll +
    // setMutedCategories pair fired two).
    expect(m.update).toHaveBeenCalledTimes(1);
    expect(m.update).toHaveBeenLastCalledWith(
      "fallow.diagnosticFilter.v1",
      expect.objectContaining({
        mutedAll: false,
        mutedCategories: ["unused-export"],
        localVisibleCategories: ["code-duplication"],
      })
    );
  });

  it("is a no-op (no write) when the selection already matches state", async () => {
    const m = memento();
    const f = new DiagnosticFilter(m as never);
    f.applyMuteSelection(false, new Set());
    await flushPersistence();
    expect(m.update).not.toHaveBeenCalled();
  });
});

describe("DiagnosticFilter.flushPersist", () => {
  it("resolves after the in-flight persisted write lands", async () => {
    const m = memento();
    const f = new DiagnosticFilter(m as never);
    f.setCategoryMuted("code-duplication", true);
    // Await the public drain API directly (what deactivate() calls), not the
    // raw microtask helper.
    await f.flushPersist();
    expect(m.update).toHaveBeenCalledWith(
      "fallow.diagnosticFilter.v1",
      expect.objectContaining({ mutedCategories: ["code-duplication"] })
    );
  });

  it("resolves immediately when there is nothing pending", async () => {
    const m = memento();
    const f = new DiagnosticFilter(m as never);
    await expect(f.flushPersist()).resolves.toBeUndefined();
  });
});

describe("DiagnosticFilter.handleDiagnostics + refresh", () => {
  it("caches unfiltered diagnostics and forwards filtered to next", () => {
    const f = new DiagnosticFilter(memento() as never);
    f.setCategoryMuted("code-duplication", true);
    const next = vi.fn();
    f.handleDiagnostics(
      fakeUri("file:///a.ts") as never,
      [diag({ code: "code-duplication" }), diag({ code: "unused-export" })] as never,
      next as never
    );
    expect(next).toHaveBeenCalledTimes(1);
    const passed = next.mock.calls[0]?.[1] as FakeDiag[];
    expect(passed).toHaveLength(1);
    expect(passed[0]?.code).toBe("unused-export");
  });

  it("refresh re-applies the new filter through client.diagnostics.set", () => {
    const f = new DiagnosticFilter(memento() as never);
    const c = collection();
    f.attachClient({ diagnostics: c as never });
    const next = vi.fn();
    f.handleDiagnostics(
      fakeUri("file:///a.ts") as never,
      [diag({ code: "code-duplication" }), diag({ code: "unused-export" })] as never,
      next as never
    );
    c.sets.length = 0;
    f.setCategoryMuted("code-duplication", true);
    const lastCall = c.sets[c.sets.length - 1];
    expect(lastCall?.uri).toBe("file:///a.ts");
    expect(lastCall?.diags).toHaveLength(1);
    expect(lastCall?.diags[0]?.code).toBe("unused-export");
    f.clearAllMutes();
    const cleared = c.sets[c.sets.length - 1];
    expect(cleared?.diags).toHaveLength(2);
  });

  it("preserves code/data/tags when re-rendering severity", () => {
    // The LSP correlates "Fix all" / delete-file code actions by
    // range + message + code, and rides changedSince / security triage facts in
    // `data`. The severity re-render must not drop those fields.
    let severity: "warning" | "hint" = "warning";
    const f = new DiagnosticFilter(memento() as never, () => severity);
    severity = "hint";
    const d = {
      source: "fallow",
      code: "unused-export",
      message: "unused export",
      severity: 1,
      data: { changedSince: "origin/main" },
      tags: [1],
      relatedInformation: [{ message: "ref" }],
    };
    const out = f.applyFilter([d] as never) as unknown as Array<typeof d>;
    expect(out).toHaveLength(1);
    expect(out[0]?.severity).toBe(3); // Hint
    expect(out[0]?.code).toBe("unused-export");
    expect(out[0]?.data).toEqual({ changedSince: "origin/main" });
    expect(out[0]?.tags).toEqual([1]);
    expect(out[0]?.relatedInformation).toEqual([{ message: "ref" }]);
  });

  it("refresh re-applies a changed severity from cached diagnostics", () => {
    let severity: "warning" | "hint" = "warning";
    const f = new DiagnosticFilter(memento() as never, () => severity);
    const c = collection();
    f.attachClient({ diagnostics: c as never });
    f.handleDiagnostics(
      fakeUri("file:///a.ts") as never,
      [diag({ code: "code-duplication", severity: 1 })] as never,
      vi.fn()
    );
    c.sets.length = 0;
    severity = "hint";
    f.refresh();
    const hinted = c.sets[c.sets.length - 1];
    expect(hinted?.diags[0]?.severity).toBe(3);
    severity = "warning";
    f.refresh();
    const warning = c.sets[c.sets.length - 1];
    expect(warning?.diags[0]?.severity).toBe(1);
  });

  it("refreshes cached diagnostics when the workspace baseline changes", () => {
    const f = new DiagnosticFilter(memento() as never);
    const c = collection();
    f.attachClient({ diagnostics: c as never });
    f.handleDiagnostics(
      fakeUri("file:///a.ts") as never,
      [diag({ code: "code-duplication" }), diag({ code: "unused-export" })] as never,
      vi.fn()
    );

    c.sets.length = 0;
    f.updateBaselineMutedCategories(new Set(["code-duplication"]));

    const lastCall = c.sets[c.sets.length - 1];
    expect(lastCall?.diags.map((d) => d.code)).toEqual(["unused-export"]);
  });

  it("caps the cache so a workspace-wide publish does not grow heap forever", () => {
    const f = new DiagnosticFilter(memento() as never);
    const c = collection();
    f.attachClient({ diagnostics: c as never });
    // Beat MAX_CACHE_ENTRIES (5000). Use a small factor over the cap to
    // confirm eviction without making the test slow.
    const overflow = 5050;
    const next = vi.fn();
    for (let i = 0; i < overflow; i++) {
      f.handleDiagnostics(
        fakeUri(`file:///f${i}.ts`) as never,
        [diag({ code: "code-duplication" })] as never,
        next as never
      );
    }
    c.sets.length = 0;
    f.setMutedAll(true);
    // The cap is 5000; refresh should touch at most 5000 URIs.
    expect(c.sets.length).toBeLessThanOrEqual(5000);
    expect(c.sets.length).toBeGreaterThan(0);
  });

  it("refresh re-pulls open documents so pull-mode diagnostics update on a toggle", () => {
    const f = new DiagnosticFilter(memento() as never);
    const c = collection();
    const refreshPullDiagnostics = vi.fn();
    f.attachClient({ diagnostics: c as never, refreshPullDiagnostics });
    // attachClient -> refresh once on attach.
    expect(refreshPullDiagnostics).toHaveBeenCalledTimes(1);
    refreshPullDiagnostics.mockClear();

    f.setMutedAll(true);
    expect(refreshPullDiagnostics).toHaveBeenCalledTimes(1);
    f.setMutedAll(false);
    expect(refreshPullDiagnostics).toHaveBeenCalledTimes(2);
    f.setCategoryMuted("code-duplication", true);
    expect(refreshPullDiagnostics).toHaveBeenCalledTimes(3);
    f.clearAllMutes();
    expect(refreshPullDiagnostics).toHaveBeenCalledTimes(4);
  });

  it("refresh re-pulls even when the push collection is absent (pure pull mode)", () => {
    const f = new DiagnosticFilter(memento() as never);
    const refreshPullDiagnostics = vi.fn();
    // No `diagnostics`: a pull-only client has no push DiagnosticCollection.
    f.attachClient({ refreshPullDiagnostics });
    refreshPullDiagnostics.mockClear();
    f.setMutedAll(true);
    expect(refreshPullDiagnostics).toHaveBeenCalledTimes(1);
  });

  it("refresh tolerates a client without a pull-refresh hook (push-only)", () => {
    const f = new DiagnosticFilter(memento() as never);
    const c = collection();
    f.attachClient({ diagnostics: c as never });
    f.handleDiagnostics(
      fakeUri("file:///a.ts") as never,
      [diag({ code: "code-duplication" }), diag({ code: "unused-export" })] as never,
      vi.fn()
    );
    c.sets.length = 0;
    expect(() => f.setMutedAll(true)).not.toThrow();
    // Push collection still re-published when no pull hook is present.
    expect(c.sets[c.sets.length - 1]?.diags).toHaveLength(0);
  });

  it("evictUri drops the cached entry so refresh stops touching it", () => {
    const f = new DiagnosticFilter(memento() as never);
    const c = collection();
    f.attachClient({ diagnostics: c as never });
    f.handleDiagnostics(
      fakeUri("file:///a.ts") as never,
      [diag({ code: "code-duplication" })] as never,
      vi.fn()
    );
    f.evictUri(fakeUri("file:///a.ts") as never);
    c.sets.length = 0;
    f.setMutedAll(true);
    expect(c.sets).toHaveLength(0);
  });

  it("handleDiagnostics does not affect other extensions' diagnostics", () => {
    const f = new DiagnosticFilter(memento() as never);
    f.setMutedAll(true);
    const next = vi.fn();
    f.handleDiagnostics(
      fakeUri("file:///a.ts") as never,
      [
        diag({ source: "ts", code: "2304" }),
        diag({ source: "eslint", code: "no-unused-vars" }),
      ] as never,
      next as never
    );
    expect(next).toHaveBeenCalledTimes(1);
    const passed = next.mock.calls[0]?.[1] as FakeDiag[];
    expect(passed).toHaveLength(2);
  });
});

describe("DiagnosticFilter pull-mode middleware", () => {
  it("filters items on a full report", async () => {
    const f = new DiagnosticFilter(memento() as never);
    f.setCategoryMuted("code-duplication", true);
    const next = vi.fn(async () => ({
      kind: "full",
      items: [
        diag({ code: "code-duplication" }),
        diag({ code: "unused-export" }),
      ],
    }));
    const result = await f.provideDiagnostics(
      fakeUri("file:///a.ts") as never,
      undefined,
      {} as never,
      next as never
    );
    expect((result as { kind: string }).kind).toBe("full");
    expect(((result as { items: FakeDiag[] }).items)).toHaveLength(1);
  });

  it("passes unchanged reports through untouched", async () => {
    const f = new DiagnosticFilter(memento() as never);
    f.setMutedAll(true);
    const next = vi.fn(async () => ({ kind: "unchanged", resultId: "r1" }));
    const result = await f.provideDiagnostics(
      fakeUri("file:///a.ts") as never,
      "r0",
      {} as never,
      next as never
    );
    expect((result as { kind: string }).kind).toBe("unchanged");
  });

  it("does not cache pull results, so refresh never duplicates open-file diagnostics into the push collection", async () => {
    const f = new DiagnosticFilter(memento() as never);
    const c = collection();
    const refreshPullDiagnostics = vi.fn();
    f.attachClient({ diagnostics: c as never, refreshPullDiagnostics });
    // An open file delivered via the pull path.
    await f.provideDiagnostics(
      fakeUri("file:///open.ts") as never,
      undefined,
      {} as never,
      vi.fn(async () => ({
        kind: "full",
        items: [diag({ code: "unused-export" })],
      })) as never
    );
    c.sets.length = 0;
    refreshPullDiagnostics.mockClear();
    // A mute toggle must re-pull (so pull-mode squiggles update) but must NOT
    // write the pulled open file into the push collection: the pull provider
    // owns a separate collection, so a push re-publish would render it twice.
    f.setMutedAll(true);
    expect(refreshPullDiagnostics).toHaveBeenCalledTimes(1);
    expect(c.sets.some((s) => s.uri === "file:///open.ts")).toBe(false);
  });

  it("refresh re-publishes push-delivered files but leaves pull-delivered files to the re-pull", async () => {
    const f = new DiagnosticFilter(memento() as never);
    const c = collection();
    f.attachClient({ diagnostics: c as never, refreshPullDiagnostics: vi.fn() });
    // Push-delivered (e.g. package.json unlisted-dependency, never opened).
    f.handleDiagnostics(
      fakeUri("file:///package.json") as never,
      [diag({ code: "unlisted-dependency" })] as never,
      vi.fn()
    );
    // Pull-delivered (an open source file).
    await f.provideDiagnostics(
      fakeUri("file:///open.ts") as never,
      undefined,
      {} as never,
      vi.fn(async () => ({
        kind: "full",
        items: [diag({ code: "unused-export" })],
      })) as never
    );
    c.sets.length = 0;
    f.setMutedAll(true);
    const uris = c.sets.map((s) => s.uri);
    expect(uris).toContain("file:///package.json");
    expect(uris).not.toContain("file:///open.ts");
  });
});

describe("DiagnosticFilter onDidChange", () => {
  it("emits on toggle changes only", () => {
    const f = new DiagnosticFilter(memento() as never);
    const events: number[] = [];
    f.onDidChange(() => events.push(1));
    f.setCategoryMuted("code-duplication", true);
    f.setCategoryMuted("code-duplication", true); // no-op
    f.setCategoryMuted("code-duplication", false);
    f.setMutedAll(false); // no-op
    expect(events).toHaveLength(2);
  });
});

describe("DIAGNOSTIC_CATEGORIES", () => {
  it("contains code-duplication first (user-facing default ordering)", () => {
    expect(DIAGNOSTIC_CATEGORIES[0]?.code).toBe("code-duplication");
  });

  it("includes every diagnostic code emitted by fallow-lsp", () => {
    // Fallback list for older LSPs. Keep it in sync with
    // `DIAGNOSTIC_ISSUE_TYPES` / `fallow/issueTypes` in `crates/lsp/src/main.rs`
    // plus any diagnostics emitted outside the issue-type catalog.
    const expected = [
      "unused-file",
      "unused-export",
      "unused-type",
      "private-type-leak",
      "unused-dependency",
      "unused-dev-dependency",
      "unused-optional-dependency",
      "unused-enum-member",
      "unused-class-member",
      "unresolved-import",
      "unlisted-dependency",
      "duplicate-export",
      "type-only-dependency",
      "test-only-dependency",
      "circular-dependency",
      "stale-suppression",
      "code-duplication",
      "boundary-violation",
    ];
    const actual = new Set(DIAGNOSTIC_CATEGORIES.map((c) => c.code));
    for (const code of expected) {
      expect(actual.has(code)).toBe(true);
    }
  });
});

describe("dynamic diagnostic categories", () => {
  it("parses the fallow/issueTypes response shape", () => {
    const parsed = parseDiagnosticCategories([
      { code: "future-rule", label: "Future Rule" },
      { code: "unused-export", label: "Unused Exports" },
    ]);
    expect(parsed).toEqual([
      { code: "future-rule", label: "Future Rule" },
      { code: "unused-export", label: "Unused Exports" },
    ]);
  });

  it("rejects malformed fallow/issueTypes responses", () => {
    expect(parseDiagnosticCategories(null)).toBeNull();
    expect(parseDiagnosticCategories([])).toBeNull();
    expect(parseDiagnosticCategories([{ code: "missing-label" }])).toBeNull();
  });

  it("updates and resets the active category catalog", () => {
    setDiagnosticCategories([{ code: "future-rule", label: "Future Rule" }]);
    expect(getDiagnosticCategories()).toEqual([
      { code: "future-rule", label: "Future Rule" },
    ]);

    resetDiagnosticCategories();
    expect(getDiagnosticCategories()).toBe(DIAGNOSTIC_CATEGORIES);
  });
});
