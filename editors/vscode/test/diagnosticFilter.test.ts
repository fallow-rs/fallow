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
