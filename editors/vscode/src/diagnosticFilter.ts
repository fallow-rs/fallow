// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import type {
  HandleDiagnosticsSignature,
  ProvideDiagnosticSignature,
  vsdiag,
} from "vscode-languageclient/node.js";
import type { DiagnosticSeveritySetting } from "./types.js";

const STATE_KEY = "fallow.diagnosticFilter.v1";
const FALLOW_SOURCE = "fallow";

/**
 * Cap the per-URI cache so a workspace-wide LSP publish on a 50k-file
 * monorepo doesn't grow the heap forever. The cache holds only PUSH-delivered
 * diagnostics (`handleDiagnostics`); fallow-lsp pushes diagnostics for every
 * diagnosed unopened file, not just open editors, and `onDidCloseTextDocument`
 * never fires for files that were never opened. (Pull results are not cached;
 * see `provideDiagnostics`.) When the cap is hit we evict the oldest entry
 * (insertion order, the first key in the Map).
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
  { code: "unused-store-member", label: "Unused Store Members" },
  { code: "unused-server-action", label: "Unused Server Actions" },
  { code: "unused-load-data-key", label: "Unused Load Data Keys" },
  { code: "unused-component-prop", label: "Unused Component Props" },
  { code: "unused-component-emit", label: "Unused Component Emits" },
  { code: "unused-component-input", label: "Unused Component Inputs" },
  { code: "unused-component-output", label: "Unused Component Outputs" },
  { code: "unrendered-component", label: "Unrendered Components" },
  { code: "unprovided-inject", label: "Unprovided Injects" },
  { code: "invalid-client-export", label: "Invalid Client Exports" },
  {
    code: "mixed-client-server-barrel",
    label: "Mixed Client/Server Barrels",
  },
  { code: "misplaced-directive", label: "Misplaced Directives" },
  { code: "route-collision", label: "Route Collisions" },
  {
    code: "dynamic-segment-name-conflict",
    label: "Dynamic Segment Name Conflicts",
  },
  { code: "unresolved-import", label: "Unresolved Imports" },
  { code: "unlisted-dependency", label: "Unlisted Dependencies" },
  { code: "duplicate-export", label: "Duplicate Exports" },
  { code: "type-only-dependency", label: "Type-Only Dependencies" },
  { code: "test-only-dependency", label: "Test-Only Dependencies" },
  { code: "circular-dependency", label: "Circular Dependencies" },
  { code: "re-export-cycle", label: "Re-Export Cycles" },
  { code: "boundary-violation", label: "Boundary Violations" },
  { code: "policy-violation", label: "Policy Violations" },
  { code: "stale-suppression", label: "Stale Suppressions" },
  { code: "unused-catalog-entry", label: "Unused Catalog Entries" },
  { code: "empty-catalog-group", label: "Empty Catalog Groups" },
  {
    code: "unresolved-catalog-reference",
    label: "Unresolved Catalog References",
  },
  {
    code: "unused-dependency-override",
    label: "Unused Dependency Overrides",
  },
  {
    code: "misconfigured-dependency-override",
    label: "Misconfigured Dependency Overrides",
  },
];

let activeDiagnosticCategories: ReadonlyArray<DiagnosticCategory> = DIAGNOSTIC_CATEGORIES;

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
  value: unknown,
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

export const setDiagnosticCategories = (categories: ReadonlyArray<DiagnosticCategory>): void => {
  if (categories.length === 0) {
    return;
  }
  activeDiagnosticCategories = categories.slice();
};

export const resetDiagnosticCategories = (): void => {
  activeDiagnosticCategories = DIAGNOSTIC_CATEGORIES;
};

export const getDiagnosticCategories = (): ReadonlyArray<DiagnosticCategory> =>
  activeDiagnosticCategories;

interface PersistedState {
  readonly mutedAll?: boolean;
  readonly mutedCategories?: ReadonlyArray<string>;
  readonly localMutedCategories?: ReadonlyArray<string>;
  readonly localVisibleCategories?: ReadonlyArray<string>;
}

interface FilterClient {
  readonly diagnostics?: vscode.DiagnosticCollection;
  /**
   * Force VS Code to re-pull `textDocument/diagnostic` for every open
   * document. The push re-publish in `refresh()` only reaches diagnostics the
   * server PUSHES (unopened files such as `package.json`); open documents
   * arrive via the LSP 3.17 pull path and are owned by vscode-languageclient's
   * internal pull collection, which `client.diagnostics.set` cannot touch. A
   * mute toggle changes only the client-side filter (no server round-trip), so
   * without an explicit re-pull the new filter state never reaches open-file
   * squiggles until the next edit. Absent for push-only clients.
   */
  readonly refreshPullDiagnostics?: () => void;
}

type DiagnosticSeverityGetter = () => DiagnosticSeveritySetting;

/** LSP diagnostics get tagged with `source: "fallow"` (see
 *  `crates/lsp/src/diagnostics/*.rs`). Anything else flows through
 *  the filter untouched so we never affect TypeScript or ESLint. */
export const isFallowDiagnostic = (d: vscode.Diagnostic): boolean => d.source === FALLOW_SOURCE;

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

export const severityToDiagnosticSeverity = (
  severity: DiagnosticSeveritySetting,
): vscode.DiagnosticSeverity => {
  switch (severity) {
    case "hint":
      return vscode.DiagnosticSeverity.Hint;
    case "information":
      return vscode.DiagnosticSeverity.Information;
    case "warning":
      return vscode.DiagnosticSeverity.Warning;
  }
};

const withRenderedSeverity = (
  d: vscode.Diagnostic,
  severity: vscode.DiagnosticSeverity,
): vscode.Diagnostic => {
  if (!isFallowDiagnostic(d) || d.severity === severity) {
    return d;
  }
  return { ...d, severity };
};

interface DiagnosticFilterStateChange {
  readonly mutedAll: boolean;
  readonly mutedCategories: ReadonlySet<string>;
}

/** Coerce a persisted value to a string array, tolerating a corrupt or
 *  hand-edited `workspaceState` entry (a non-array, or an array with non-string
 *  members). Without this a bad value (e.g. a number where an array is expected)
 *  throws in the constructor and disables the whole extension for that
 *  workspace. */
const asStringArray = (value: unknown): string[] =>
  Array.isArray(value) ? value.filter((item): item is string => typeof item === "string") : [];

export class DiagnosticFilter {
  private mutedAll = false;
  private baselineMutedCategories = new Set<string>();
  private localMutedCategories = new Set<string>();
  private localVisibleCategories = new Set<string>();
  private readonly cache = new Map<string, vscode.Diagnostic[]>();
  private client: FilterClient | null = null;
  private persistQueue: Promise<void> = Promise.resolve();
  private readonly emitter = new vscode.EventEmitter<DiagnosticFilterStateChange>();

  public readonly onDidChange = this.emitter.event;

  public constructor(
    private readonly memento: vscode.Memento,
    private readonly getSeverity: DiagnosticSeverityGetter = () => "warning",
    baselineMutedCategories: ReadonlySet<string> = new Set(),
  ) {
    this.baselineMutedCategories = new Set(baselineMutedCategories);
    const persisted = memento.get<PersistedState>(STATE_KEY);
    if (persisted && typeof persisted === "object") {
      this.mutedAll = persisted.mutedAll === true;
      const localMuted = asStringArray(persisted.localMutedCategories ?? persisted.mutedCategories);
      this.localMutedCategories = new Set(localMuted);
      this.localVisibleCategories = new Set(asStringArray(persisted.localVisibleCategories));
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

  /** Await any in-flight persisted-state write. `dispose()` is synchronous (the
   *  VS Code Disposable contract), so it cannot await the persist queue; the
   *  extension's async `deactivate()` calls this so the last mute toggle is not
   *  dropped when the window closes mid-write. */
  public async flushPersist(): Promise<void> {
    await this.persistQueue;
  }

  public isMutedAll(): boolean {
    return this.mutedAll;
  }

  public isCategoryMuted(code: string): boolean {
    return this.effectiveMutedCategories().has(code);
  }

  public anythingMuted(): boolean {
    return this.mutedAll || this.effectiveMutedCategories().size > 0;
  }

  public mutedCategoriesSnapshot(): ReadonlySet<string> {
    return this.effectiveMutedCategories();
  }

  public updateBaselineMutedCategories(codes: ReadonlySet<string>): void {
    const next = new Set(codes);
    if (setsEqual(this.baselineMutedCategories, next)) {
      return;
    }
    this.baselineMutedCategories = next;
    this.pruneVisibleOverrides();
    this.persist();
    this.refresh();
    this.emitChange();
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
    const had = this.isCategoryMuted(code);
    if (value === had) {
      return;
    }
    if (value) {
      this.localVisibleCategories.delete(code);
      if (!this.baselineMutedCategories.has(code)) {
        this.localMutedCategories.add(code);
      }
    } else {
      this.localMutedCategories.delete(code);
      if (this.baselineMutedCategories.has(code)) {
        this.localVisibleCategories.add(code);
      }
    }
    this.persist();
    this.refresh();
    this.emitChange();
  }

  public setMutedCategories(codes: ReadonlySet<string>): void {
    const nextLocalMuted = new Set<string>();
    const nextLocalVisible = new Set<string>();
    for (const code of codes) {
      if (!this.baselineMutedCategories.has(code)) {
        nextLocalMuted.add(code);
      }
    }
    for (const code of this.baselineMutedCategories) {
      if (!codes.has(code)) {
        nextLocalVisible.add(code);
      }
    }
    const changed =
      !setsEqual(this.localMutedCategories, nextLocalMuted) ||
      !setsEqual(this.localVisibleCategories, nextLocalVisible);
    if (!changed) {
      return;
    }

    this.localMutedCategories = nextLocalMuted;
    this.localVisibleCategories = nextLocalVisible;
    this.persist();
    this.refresh();
    this.emitChange();
  }

  /** Apply the global mute-all flag AND an explicit category set in ONE
   *  persist/refresh/emit cycle. The Manage quick pick otherwise called
   *  setMutedAll then setMutedCategories for a single accept, firing two
   *  persisted writes and two refreshes (two LSP re-pulls) per user action. */
  public applyMuteSelection(mutedAll: boolean, codes: ReadonlySet<string>): void {
    const nextLocalMuted = new Set<string>();
    const nextLocalVisible = new Set<string>();
    for (const code of codes) {
      if (!this.baselineMutedCategories.has(code)) {
        nextLocalMuted.add(code);
      }
    }
    for (const code of this.baselineMutedCategories) {
      if (!codes.has(code)) {
        nextLocalVisible.add(code);
      }
    }
    const changed =
      this.mutedAll !== mutedAll ||
      !setsEqual(this.localMutedCategories, nextLocalMuted) ||
      !setsEqual(this.localVisibleCategories, nextLocalVisible);
    if (!changed) {
      return;
    }
    this.mutedAll = mutedAll;
    this.localMutedCategories = nextLocalMuted;
    this.localVisibleCategories = nextLocalVisible;
    this.persist();
    this.refresh();
    this.emitChange();
  }

  public toggleCategory(code: string): boolean {
    const next = !this.isCategoryMuted(code);
    this.setCategoryMuted(code, next);
    return next;
  }

  public clearAllMutes(): void {
    if (!this.anythingMuted()) {
      return;
    }
    this.mutedAll = false;
    this.localMutedCategories.clear();
    this.localVisibleCategories = new Set(this.baselineMutedCategories);
    this.persist();
    this.refresh();
    this.emitChange();
  }

  /** Drop the cache entry for a closed document so we don't grow unbounded
   *  on large monorepos. The LSP will re-publish if it reopens. */
  public evictUri(uri: vscode.Uri): void {
    this.cache.delete(uri.toString());
  }

  public applyFilter(diagnostics: ReadonlyArray<vscode.Diagnostic>): vscode.Diagnostic[] {
    const renderedSeverity = severityToDiagnosticSeverity(this.getSeverity());
    const filtered = this.anythingMuted()
      ? diagnostics.filter((d) => {
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
          return !this.isCategoryMuted(code);
        })
      : diagnostics;
    return filtered.map((d) => withRenderedSeverity(d, renderedSeverity));
  }

  /** Push-mode middleware: intercepts `textDocument/publishDiagnostics`. */
  public handleDiagnostics(
    uri: vscode.Uri,
    diagnostics: vscode.Diagnostic[],
    next: HandleDiagnosticsSignature,
  ): void {
    const key = uri.toString();
    this.evictIfFull(key);
    this.cache.set(key, diagnostics.slice());
    next(uri, this.applyFilter(diagnostics));
  }

  /** Pull-mode middleware: intercepts `textDocument/diagnostic`. The LSP
   *  advertises `diagnostic_provider` in `build_server_capabilities()`, so
   *  VS Code and strict 3.17 clients can hit this path.
   *
   *  Pull results are deliberately NOT cached. The pull path re-fetches from
   *  the server on every re-pull (a mute toggle triggers one via
   *  `refreshPullDiagnostics`), so a cached copy is never read back. Worse, the
   *  pull provider owns its OWN `DiagnosticCollection` (named after the server's
   *  diagnostic `identifier`), which is DISTINCT from the push collection
   *  (`client.diagnostics`) that `refresh()` re-publishes into. Caching an
   *  open-file pull result would let `refresh()` write it into the push
   *  collection too, rendering every open-file squiggle TWICE after a toggle.
   *  Only PUSH-delivered diagnostics (`handleDiagnostics`, for unopened files
   *  such as `package.json`) belong in the cache. */
  public async provideDiagnostics(
    document: vscode.TextDocument | vscode.Uri,
    previousResultId: string | undefined,
    token: vscode.CancellationToken,
    next: ProvideDiagnosticSignature,
  ): Promise<vsdiag.DocumentDiagnosticReport | undefined | null> {
    const result = await next(document, previousResultId, token);
    if (!result) {
      return result;
    }
    if (result.kind !== "full") {
      return result;
    }
    return { ...result, items: this.applyFilter(result.items) };
  }

  /** Re-apply the current filter so squiggles update instantly on a toggle
   *  change without an LSP restart or re-analysis. Two delivery paths need
   *  refreshing: the PUSH collection (re-published in place from the cache,
   *  covering unopened-file diagnostics such as `package.json`), and the PULL
   *  path (open documents), which is owned by vscode-languageclient and can
   *  only be updated by asking the client to re-pull. Snapshots entries first
   *  to future-proof against async creep in callers. */
  public refresh(): void {
    const client = this.client;
    if (!client) {
      return;
    }
    const collection = client.diagnostics;
    if (collection) {
      const entries = Array.from(this.cache.entries());
      for (const [uriStr, diagnostics] of entries) {
        collection.set(vscode.Uri.parse(uriStr), this.applyFilter(diagnostics));
      }
    }
    // Re-pull open documents so the new filter state reaches pull-mode
    // (open-file) squiggles immediately, not just on the next edit. Without
    // this, undoing "hide all findings" leaves open files stuck hidden.
    client.refreshPullDiagnostics?.();
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
    const effectiveMutedCategories = Array.from(this.effectiveMutedCategories());
    const payload: PersistedState = {
      mutedAll: this.mutedAll,
      mutedCategories: effectiveMutedCategories,
      localMutedCategories: Array.from(this.localMutedCategories),
      localVisibleCategories: Array.from(this.localVisibleCategories),
    };
    this.persistQueue = this.persistQueue.then(
      () => Promise.resolve(this.memento.update(STATE_KEY, payload)),
      () => Promise.resolve(this.memento.update(STATE_KEY, payload)),
    );
  }

  private emitChange(): void {
    this.emitter.fire({
      mutedAll: this.mutedAll,
      mutedCategories: this.mutedCategoriesSnapshot(),
    });
  }

  private effectiveMutedCategories(): Set<string> {
    const result = new Set(this.baselineMutedCategories);
    for (const code of this.localMutedCategories) {
      result.add(code);
    }
    for (const code of this.localVisibleCategories) {
      result.delete(code);
    }
    return result;
  }

  private pruneVisibleOverrides(): void {
    for (const code of Array.from(this.localVisibleCategories)) {
      if (!this.baselineMutedCategories.has(code)) {
        this.localVisibleCategories.delete(code);
      }
    }
  }
}

const setsEqual = (left: ReadonlySet<string>, right: ReadonlySet<string>): boolean => {
  if (left.size !== right.size) {
    return false;
  }
  for (const value of left) {
    if (!right.has(value)) {
      return false;
    }
  }
  return true;
};
