// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import type {
  HandleDiagnosticsSignature,
  ProvideDiagnosticSignature,
  vsdiag,
} from "vscode-languageclient/node.js";

const STATE_KEY = "fallow.diagnosticFilter.v1";
const FALLOW_SOURCE = "fallow";

/**
 * Cap the per-URI cache so a workspace-wide LSP publish on a 50k-file
 * monorepo doesn't grow the heap forever. fallow-lsp publishes diagnostics
 * for every diagnosed file, not just open editors, and `onDidCloseTextDocument`
 * never fires for files that were never opened. When the cap is hit we evict
 * the oldest entry (insertion order, the first key in the Map).
 */
const MAX_CACHE_ENTRIES = 5000;

export interface DiagnosticCategory {
  readonly code: string;
  readonly label: string;
}

/**
 * Fallback diagnostic categories for older fallow-lsp binaries that do not
 * support `fallow/issueTypes`. Current servers provide the canonical list.
 */
export const DIAGNOSTIC_CATEGORIES: ReadonlyArray<DiagnosticCategory> = [
  { code: "code-duplication", label: "Code Duplication" },
  { code: "unused-file", label: "Unused Files" },
  { code: "unused-export", label: "Unused Exports" },
  { code: "unused-type", label: "Unused Types" },
  { code: "private-type-leak", label: "Private Type Leaks" },
  { code: "unused-dependency", label: "Unused Dependencies" },
  { code: "unused-dev-dependency", label: "Unused Dev Dependencies" },
  {
    code: "unused-optional-dependency",
    label: "Unused Optional Dependencies",
  },
  { code: "unused-enum-member", label: "Unused Enum Members" },
  { code: "unused-class-member", label: "Unused Class Members" },
  { code: "unresolved-import", label: "Unresolved Imports" },
  { code: "unlisted-dependency", label: "Unlisted Dependencies" },
  { code: "duplicate-export", label: "Duplicate Exports" },
  { code: "type-only-dependency", label: "Type-Only Dependencies" },
  { code: "test-only-dependency", label: "Test-Only Dependencies" },
  { code: "circular-dependency", label: "Circular Dependencies" },
  { code: "boundary-violation", label: "Boundary Violations" },
  { code: "stale-suppression", label: "Stale Suppressions" },
  { code: "unused-catalog-entry", label: "Unused Catalog Entries" },
];

let activeDiagnosticCategories: ReadonlyArray<DiagnosticCategory> =
  DIAGNOSTIC_CATEGORIES;

const isDiagnosticCategory = (value: unknown): value is DiagnosticCategory => {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as { code?: unknown; label?: unknown };
  return (
    typeof candidate.code === "string" &&
    candidate.code.length > 0 &&
    typeof candidate.label === "string" &&
    candidate.label.length > 0
  );
};

export const parseDiagnosticCategories = (
  value: unknown
): ReadonlyArray<DiagnosticCategory> | null => {
  if (!Array.isArray(value)) {
    return null;
  }
  const categories = value.filter(isDiagnosticCategory);
  if (categories.length !== value.length || categories.length === 0) {
    return null;
  }
  return categories.map(({ code, label }) => ({ code, label }));
};

export const setDiagnosticCategories = (
  categories: ReadonlyArray<DiagnosticCategory>
): void => {
  if (categories.length === 0) {
    return;
  }
  activeDiagnosticCategories = categories.slice();
};

export const resetDiagnosticCategories = (): void => {
  activeDiagnosticCategories = DIAGNOSTIC_CATEGORIES;
};

export const getDiagnosticCategories =
  (): ReadonlyArray<DiagnosticCategory> => activeDiagnosticCategories;

interface PersistedState {
  readonly mutedAll?: boolean;
  readonly mutedCategories?: ReadonlyArray<string>;
}

interface FilterClient {
  readonly diagnostics?: vscode.DiagnosticCollection;
}

/** LSP diagnostics get tagged with `source: "fallow"` (see
 *  `crates/lsp/src/diagnostics/*.rs`). Anything else flows through
 *  the filter untouched so we never affect TypeScript or ESLint. */
export const isFallowDiagnostic = (d: vscode.Diagnostic): boolean =>
  d.source === FALLOW_SOURCE;

/** `Diagnostic.code` per VSCode types is `string | number | { value, target }`,
 *  and may be absent. Returns `null` when there's nothing to match against. */
export const diagnosticCode = (d: vscode.Diagnostic): string | null => {
  const code = d.code;
  if (code === undefined || code === null) {
    return null;
  }
  if (typeof code === "string") {
    return code;
  }
  if (typeof code === "number") {
    return String(code);
  }
  if (typeof code === "object" && "value" in code) {
    const value = (code as { value: string | number }).value;
    return typeof value === "string" ? value : String(value);
  }
  return null;
};

interface DiagnosticFilterStateChange {
  readonly mutedAll: boolean;
  readonly mutedCategories: ReadonlySet<string>;
}

export class DiagnosticFilter {
  private mutedAll = false;
  private mutedCategories = new Set<string>();
  private readonly cache = new Map<string, vscode.Diagnostic[]>();
  private client: FilterClient | null = null;
  private persistQueue: Promise<void> = Promise.resolve();
  private readonly emitter =
    new vscode.EventEmitter<DiagnosticFilterStateChange>();

  public readonly onDidChange = this.emitter.event;

  public constructor(private readonly memento: vscode.Memento) {
    const persisted = memento.get<PersistedState>(STATE_KEY);
    if (persisted) {
      this.mutedAll = persisted.mutedAll === true;
      const list = persisted.mutedCategories ?? [];
      this.mutedCategories = new Set(list);
    }
  }

  public attachClient(client: FilterClient): void {
    this.client = client;
    this.refresh();
  }

  public detachClient(): void {
    this.client = null;
    this.cache.clear();
  }

  public dispose(): void {
    this.emitter.dispose();
  }

  public isMutedAll(): boolean {
    return this.mutedAll;
  }

  public isCategoryMuted(code: string): boolean {
    return this.mutedCategories.has(code);
  }

  public anythingMuted(): boolean {
    return this.mutedAll || this.mutedCategories.size > 0;
  }

  public mutedCategoriesSnapshot(): ReadonlySet<string> {
    return new Set(this.mutedCategories);
  }

  public setMutedAll(value: boolean): void {
    if (this.mutedAll === value) {
      return;
    }
    this.mutedAll = value;
    this.persist();
    this.refresh();
    this.emitChange();
  }

  public toggleMutedAll(): boolean {
    this.setMutedAll(!this.mutedAll);
    return this.mutedAll;
  }

  public setCategoryMuted(code: string, value: boolean): void {
    const had = this.mutedCategories.has(code);
    if (value === had) {
      return;
    }
    if (value) {
      this.mutedCategories.add(code);
    } else {
      this.mutedCategories.delete(code);
    }
    this.persist();
    this.refresh();
    this.emitChange();
  }

  public setMutedCategories(codes: ReadonlySet<string>): void {
    let changed = this.mutedCategories.size !== codes.size;
    if (!changed) {
      for (const code of codes) {
        if (!this.mutedCategories.has(code)) {
          changed = true;
          break;
        }
      }
    }
    if (!changed) {
      return;
    }

    this.mutedCategories = new Set(codes);
    this.persist();
    this.refresh();
    this.emitChange();
  }

  public toggleCategory(code: string): boolean {
    const next = !this.mutedCategories.has(code);
    this.setCategoryMuted(code, next);
    return next;
  }

  public clearAllMutes(): void {
    if (!this.anythingMuted()) {
      return;
    }
    this.mutedAll = false;
    this.mutedCategories.clear();
    this.persist();
    this.refresh();
    this.emitChange();
  }

  /** Drop the cache entry for a closed document so we don't grow unbounded
   *  on large monorepos. The LSP will re-publish if it reopens. */
  public evictUri(uri: vscode.Uri): void {
    this.cache.delete(uri.toString());
  }

  public applyFilter(
    diagnostics: ReadonlyArray<vscode.Diagnostic>
  ): vscode.Diagnostic[] {
    if (!this.anythingMuted()) {
      return diagnostics.slice();
    }
    return diagnostics.filter((d) => {
      if (!isFallowDiagnostic(d)) {
        return true;
      }
      if (this.mutedAll) {
        return false;
      }
      const code = diagnosticCode(d);
      if (code === null) {
        return true;
      }
      return !this.mutedCategories.has(code);
    });
  }

  /** Push-mode middleware: intercepts `textDocument/publishDiagnostics`. */
  public handleDiagnostics(
    uri: vscode.Uri,
    diagnostics: vscode.Diagnostic[],
    next: HandleDiagnosticsSignature
  ): void {
    const key = uri.toString();
    this.evictIfFull(key);
    this.cache.set(key, diagnostics.slice());
    next(uri, this.applyFilter(diagnostics));
  }

  /** Pull-mode middleware: intercepts `textDocument/diagnostic`. The LSP
   *  advertises `diagnostic_provider` in `build_server_capabilities()`, so
   *  strict 3.17 clients (and a future VSCode pull flip) hit this path. */
  public async provideDiagnostics(
    document: vscode.TextDocument | vscode.Uri,
    previousResultId: string | undefined,
    token: vscode.CancellationToken,
    next: ProvideDiagnosticSignature
  ): Promise<vsdiag.DocumentDiagnosticReport | undefined | null> {
    const result = await next(document, previousResultId, token);
    if (!result) {
      return result;
    }
    if (result.kind !== "full") {
      return result;
    }
    // `document` is `TextDocument | Uri`. TextDocument exposes `.uri`;
    // a bare Uri does not. Structural detection works for both real and
    // mocked Uri objects (mocks aren't `instanceof vscode.Uri`).
    const uri =
      "uri" in document && document.uri !== undefined
        ? document.uri
        : (document as vscode.Uri);
    const key = uri.toString();
    this.evictIfFull(key);
    this.cache.set(key, result.items.slice());
    return { ...result, items: this.applyFilter(result.items) };
  }

  /** Re-apply current filter to all cached diagnostics via the client's
   *  collection. Called on toggle change so squiggles update instantly
   *  without an LSP restart or re-analysis. Snapshots entries first to
   *  future-proof against async creep in callers. */
  public refresh(): void {
    const collection = this.client?.diagnostics;
    if (!collection) {
      return;
    }
    const entries = Array.from(this.cache.entries());
    for (const [uriStr, diagnostics] of entries) {
      collection.set(vscode.Uri.parse(uriStr), this.applyFilter(diagnostics));
    }
  }

  /** Drop the oldest cache entry when at capacity, unless the URI we're
   *  about to write was already cached (in-place update doesn't grow size). */
  private evictIfFull(incomingKey: string): void {
    if (this.cache.size < MAX_CACHE_ENTRIES) {
      return;
    }
    if (this.cache.has(incomingKey)) {
      return;
    }
    const oldest = this.cache.keys().next().value;
    if (oldest !== undefined) {
      this.cache.delete(oldest);
    }
  }

  private persist(): void {
    const payload: PersistedState = {
      mutedAll: this.mutedAll,
      mutedCategories: Array.from(this.mutedCategories),
    };
    this.persistQueue = this.persistQueue.then(
      () => Promise.resolve(this.memento.update(STATE_KEY, payload)),
      () => Promise.resolve(this.memento.update(STATE_KEY, payload))
    );
  }

  private emitChange(): void {
    this.emitter.fire({
      mutedAll: this.mutedAll,
      mutedCategories: this.mutedCategoriesSnapshot(),
    });
  }
}
