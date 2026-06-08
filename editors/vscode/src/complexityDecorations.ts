import * as path from "node:path";
import * as vscode from "vscode";
import type { ComplexityContribution, ComplexityContributionKind, HealthFinding } from "./types.js";
import { resolveFilePath } from "./treeView-utils.js";

/**
 * Inline "why is this complex" editor decorations (progressive disclosure).
 *
 * The compact summary (`N cyc, N cog`) lives in the lens above each complex
 * function (see `complexityLens.ts`). The dense per-line `+N` breakdown is the
 * DETAIL tier: it renders only for functions the user has expanded, either by
 * pinning (clicking the lens) or selecting the finding in the Health view, or
 * for every function when `fallow.complexity.afterText` is on. Hover always
 * shows the breakdown in a popup regardless, via `provideHover`.
 *
 * The per-decision-point breakdown arrives on each complexity finding as a
 * `contributions[]` array (one entry per increment event, cyclomatic and
 * cognitive recorded separately). The wire shape is per-increment, but the
 * editor renders per-LINE: contributions are grouped by line and summed into a
 * single dim token, with the per-kind list deferred to the hover.
 */

/** Human label for each contribution kind, mirroring SonarSource vocabulary. */
const KIND_LABELS: Record<ComplexityContributionKind, string> = {
  if: "if",
  else: "else",
  "else-if": "else if",
  ternary: "ternary",
  "logical-and": "&&",
  "logical-or": "||",
  "nullish-coalescing": "??",
  "logical-assignment": "logical assignment",
  "optional-chain": "?.",
  for: "for loop",
  "for-in": "for…in",
  "for-of": "for…of",
  while: "while loop",
  "do-while": "do…while",
  switch: "switch",
  case: "case",
  catch: "catch",
  "labeled-break": "labeled break",
  "labeled-continue": "labeled continue",
};

const kindLabel = (kind: ComplexityContributionKind): string => KIND_LABELS[kind] ?? kind;

const roundTo = (value: number, places: number): number => {
  const factor = 10 ** places;
  return Math.round(value * factor) / factor;
};

/**
 * Explain a CRAP score from the fields already on the finding (no extra data
 * from Rust). CRAP = cc² × (1 − coverage)³ + cc, so at 0% coverage it is
 * cc² + cc, and full coverage drops it to cc. Returns `undefined` when the
 * finding carries no CRAP score.
 */
export const crapExplanation = (finding: HealthFinding): string | undefined => {
  if (finding.crap == null) {
    return undefined;
  }
  const crap = roundTo(finding.crap, 1);
  const cc = finding.cyclomatic;
  const coverage = finding.coverage_pct;
  let coverageText: string;
  if (coverage == null) {
    coverageText = "coverage unknown";
  } else if (coverage <= 0) {
    coverageText = "untested (0% covered)";
  } else if (coverage >= 100) {
    coverageText = "fully covered";
  } else {
    coverageText = `${roundTo(coverage, 0)}% covered`;
  }
  const tail = cc < crap ? ` Full test coverage would bring CRAP down to ${cc}.` : "";
  return `CRAP ${crap}: cyclomatic ${cc}, ${coverageText}.${tail}`;
};

/** A line's aggregated contribution data, ready to render. */
interface LineAggregate {
  /** 1-based source line. */
  readonly line: number;
  /** Sum of cyclomatic weights on the line. */
  readonly cyclomatic: number;
  /** Sum of cognitive weights on the line. */
  readonly cognitive: number;
  /** The contributions on the line, for the hover. */
  readonly contributions: readonly ComplexityContribution[];
}

const aggregateByLine = (findings: readonly HealthFinding[]): Map<number, LineAggregate> => {
  const byLine = new Map<number, ComplexityContribution[]>();
  for (const finding of findings) {
    for (const contribution of finding.contributions ?? []) {
      const bucket = byLine.get(contribution.line);
      if (bucket) {
        bucket.push(contribution);
      } else {
        byLine.set(contribution.line, [contribution]);
      }
    }
  }
  const result = new Map<number, LineAggregate>();
  for (const [line, contributions] of byLine) {
    let cyclomatic = 0;
    let cognitive = 0;
    for (const c of contributions) {
      if (c.metric === "cyclomatic") {
        cyclomatic += c.weight;
      } else {
        cognitive += c.weight;
      }
    }
    result.set(line, { line, cyclomatic, cognitive, contributions });
  }
  return result;
};

/**
 * The dominant construct on a line: the kind of the highest-weight contribution
 * (ties resolve to the first seen). Drives the short inline label.
 */
const dominantKind = (
  contributions: readonly ComplexityContribution[],
): ComplexityContributionKind => {
  let best = contributions[0];
  for (const c of contributions) {
    if (c.weight > best.weight) {
      best = c;
    }
  }
  return best.kind;
};

const inlineToken = (aggregate: LineAggregate): string => {
  // Cognitive is the nesting-sensitive "how hard to follow" headline; fall back
  // to cyclomatic for lines that only add independent paths (a case label, a
  // logical-assignment, an optional-chain link).
  const headline = aggregate.cognitive > 0 ? aggregate.cognitive : aggregate.cyclomatic;
  const kinds = new Set(aggregate.contributions.map((c) => c.kind));
  const label = kindLabel(dominantKind(aggregate.contributions));
  const extra = kinds.size > 1 ? ` +${kinds.size - 1}` : "";
  return `+${headline} ${label}${extra}`;
};

const lineHover = (aggregate: LineAggregate): vscode.MarkdownString => {
  const md = new vscode.MarkdownString();
  md.appendMarkdown("**Complexity contributions on this line**\n\n");
  for (const c of aggregate.contributions) {
    const nesting = c.metric === "cognitive" && c.nesting > 0 ? ` (nesting ${c.nesting})` : "";
    md.appendMarkdown(`- ${kindLabel(c.kind)} · +${c.weight} ${c.metric}${nesting}\n`);
  }
  return md;
};

const functionHover = (finding: HealthFinding): vscode.MarkdownString => {
  const md = new vscode.MarkdownString();
  md.appendMarkdown(
    `**${finding.name}** · cyclomatic ${finding.cyclomatic} · cognitive ${finding.cognitive}\n\n`,
  );
  const crap = crapExplanation(finding);
  if (crap) {
    md.appendMarkdown(`${crap}\n`);
  }
  return md;
};

const sameFile = (
  findingPath: string,
  documentPath: string,
  workspaceRoot: string | undefined,
): boolean => {
  const { absolute } = resolveFilePath(findingPath, workspaceRoot);
  if (!absolute) {
    return false;
  }
  return path.normalize(absolute) === path.normalize(documentPath);
};

/** Stable per-function key (project-relative finding path + 1-based line). */
export const complexityKey = (path: string, line: number): string => `${path}:${line}`;

/**
 * The hover markdown for a 1-based source line: the function summary + CRAP
 * when the line is a complex function's signature, the per-kind contribution
 * list when it is a decision-point line, `undefined` otherwise. Pure (the
 * markdown type is the only VS Code dependency), so it is unit-testable
 * independently of an open editor.
 */
export const hoverForLine = (
  matched: readonly HealthFinding[],
  line1Based: number,
): vscode.MarkdownString | undefined => {
  const fn = matched.find((f) => f.line === line1Based);
  if (fn) {
    return functionHover(fn);
  }
  const aggregate = aggregateByLine(matched).get(line1Based);
  if (aggregate) {
    return lineHover(aggregate);
  }
  return undefined;
};

/**
 * A single DETAIL decoration to place on a document: the target line and the
 * dim `+N kind` token. The controller anchors it at the END of the line (so the
 * text never shifts the code) once it has the document to read line lengths.
 */
export interface ComplexityDecorationSpec {
  /** 0-based line the decoration attaches to. */
  readonly line: number;
  /** Dim end-of-line token (`+N kind`). */
  readonly afterText: string;
}

/**
 * Build the per-line DETAIL decoration specs for one open document from the
 * cached health findings, limited to the functions the caller marks expanded.
 * Pure: no VS Code editor I/O, so it is directly unit-testable. The controller
 * anchors each spec at end-of-line.
 *
 * @param isExpanded returns true for findings whose per-line breakdown should
 *   render (pinned, selected, or all when the global after-text tier is on).
 */
export const buildComplexityDecorations = (
  findings: readonly HealthFinding[],
  documentPath: string,
  workspaceRoot: string | undefined,
  isExpanded: (finding: HealthFinding) => boolean,
): ComplexityDecorationSpec[] => {
  const expanded = findings.filter(
    (f) => sameFile(f.path, documentPath, workspaceRoot) && isExpanded(f),
  );
  const specs: ComplexityDecorationSpec[] = [];
  for (const aggregate of aggregateByLine(expanded).values()) {
    specs.push({ line: Math.max(0, aggregate.line - 1), afterText: inlineToken(aggregate) });
  }
  return specs;
};

/**
 * Owns the per-line detail decorations and the per-function expansion state
 * (pinned via the lens, selected via the Health view). Drives rendering across
 * the active-editor and document-change lifecycle, and serves the lens provider
 * (`findingsForDocument` + `isPinned` + `onDidChange`) and the hover provider
 * (`provideHover`).
 *
 * Line-drift fail-safe: decorations are anchored to the line numbers from the
 * LAST health run. If the user edits a decorated document before the next run,
 * the markers would point at the wrong lines, so on the first edit the document
 * is marked stale and its decorations + lenses + hovers are suppressed until
 * fresh findings arrive (never best-effort re-anchored: a marker on the wrong
 * branch misleads).
 */
export class ComplexityDecorationController {
  private readonly contributionType: vscode.TextEditorDecorationType;
  private findings: readonly HealthFinding[] = [];
  private readonly staleDocuments = new Set<string>();
  /** Functions pinned-open by clicking their lens (persistent, key = path:line). */
  private readonly pinned = new Set<string>();
  /** The function whose Health-view finding is currently selected (transient). */
  private selected: string | undefined;
  private readonly changeEmitter = new vscode.EventEmitter<void>();
  /**
   * Fires when the rendered/expanded set changes (new findings, pin toggle,
   * selection, staleness). The lens provider subscribes via
   * `onDidChangeCodeLenses` so titles flip and lenses clear in step.
   */
  readonly onDidChange = this.changeEmitter.event;

  constructor(
    private readonly isEnabled: () => boolean,
    private readonly showAllAfterText: () => boolean,
    private readonly workspaceRoot: () => string | undefined,
  ) {
    this.contributionType = vscode.window.createTextEditorDecorationType({});
  }

  /** Adopt the findings from a completed health run and re-render. */
  setFindings(findings: readonly HealthFinding[]): void {
    this.findings = findings;
    this.staleDocuments.clear();
    this.renderVisibleEditors();
    this.changeEmitter.fire();
  }

  /**
   * Re-render decorations and refresh lenses after a relevant setting change
   * (`fallow.health.inlineComplexity`), without respawning the health analysis.
   */
  refresh(): void {
    this.renderVisibleEditors();
    this.changeEmitter.fire();
  }

  /** A document edit may have shifted line numbers: clear + mark stale. */
  handleDocumentChange(document: vscode.TextDocument): void {
    const key = document.uri.toString();
    if (this.staleDocuments.has(key)) {
      return;
    }
    this.staleDocuments.add(key);
    for (const editor of vscode.window.visibleTextEditors) {
      if (editor.document.uri.toString() === key) {
        this.clear(editor);
      }
    }
    // Suppress the lens too while the document is stale.
    this.changeEmitter.fire();
  }

  /** True when a document was edited since the last health run (markers stale). */
  isStale(document: vscode.TextDocument): boolean {
    return this.staleDocuments.has(document.uri.toString());
  }

  /** Findings whose function lives in the given document (for lens + hover). */
  findingsForDocument(documentPath: string): readonly HealthFinding[] {
    return this.findings.filter((f) => sameFile(f.path, documentPath, this.workspaceRoot()));
  }

  /**
   * Whether the function at `path:line` is currently expanded by EITHER a pin
   * (lens click) or the Health-view selection. This is the single state the
   * lens title reflects (`show` vs `hide breakdown`), so the sidebar selection
   * and the lens toggle never disagree: selecting a finding makes its lens read
   * `hide breakdown` because the breakdown is already showing.
   */
  isExpanded(filePath: string, line: number): boolean {
    return this.isExpandedKey(complexityKey(filePath, line));
  }

  /**
   * Toggle the unified expansion for one function (the lens click). If it is
   * expanded by anything (pin OR selection), collapse it fully: drop the pin
   * AND clear the selection contribution, so clicking `hide breakdown` on a
   * selected function hides it (and it stays hidden until selected again).
   * Otherwise pin it open.
   */
  toggleExpanded(filePath: string, line: number): void {
    const key = complexityKey(filePath, line);
    if (this.isExpandedKey(key)) {
      this.pinned.delete(key);
      if (this.selected === key) {
        this.selected = undefined;
      }
    } else {
      this.pinned.add(key);
    }
    this.renderVisibleEditors();
    this.changeEmitter.fire();
  }

  private isExpandedKey(key: string): boolean {
    return this.pinned.has(key) || this.selected === key;
  }

  /**
   * Set (or clear) the transiently-expanded function driven by Health-view
   * selection. Composes with pins: the rendered set is `pinned ∪ selected`.
   */
  setSelectedFunction(target: { readonly path: string; readonly line: number } | undefined): void {
    const key = target ? complexityKey(target.path, target.line) : undefined;
    if (this.selected === key) {
      return;
    }
    this.selected = key;
    this.renderVisibleEditors();
    this.changeEmitter.fire();
  }

  /** Hover for a position: function summary or per-line breakdown popup. */
  provideHover(document: vscode.TextDocument, position: vscode.Position): vscode.Hover | undefined {
    if (!this.isEnabled() || document.uri.scheme !== "file" || this.isStale(document)) {
      return undefined;
    }
    const matched = this.findingsForDocument(document.uri.fsPath);
    if (matched.length === 0) {
      return undefined;
    }
    const markdown = hoverForLine(matched, position.line + 1);
    return markdown ? new vscode.Hover(markdown) : undefined;
  }

  /** Whether a finding's per-line detail should render (forced-on, pin, or selection). */
  private shouldRenderDetail(finding: HealthFinding): boolean {
    return (
      this.showAllAfterText() || this.isExpandedKey(complexityKey(finding.path, finding.line))
    );
  }

  /** Render (or clear) one editor's per-line detail from the cached findings. */
  renderEditor(editor: vscode.TextEditor | undefined): void {
    if (!editor) {
      return;
    }
    if (!this.isEnabled() || this.isStale(editor.document) || editor.document.uri.scheme !== "file") {
      this.clear(editor);
      return;
    }
    const specs = buildComplexityDecorations(
      this.findings,
      editor.document.uri.fsPath,
      this.workspaceRoot(),
      (finding) => this.shouldRenderDetail(finding),
    );
    editor.setDecorations(this.contributionType, this.toOptions(editor.document, specs));
  }

  /**
   * Anchor each spec at the END of its line so the `afterText` renders in the
   * empty space past the code (never shifting the code right). Lines outside the
   * current document (the file was edited shorter) are clamped and skipped.
   */
  private toOptions(
    document: vscode.TextDocument,
    specs: readonly ComplexityDecorationSpec[],
  ): vscode.DecorationOptions[] {
    const options: vscode.DecorationOptions[] = [];
    for (const spec of specs) {
      if (spec.line >= document.lineCount) {
        continue;
      }
      const end = document.lineAt(spec.line).range.end;
      options.push({
        range: new vscode.Range(end, end),
        renderOptions: {
          after: {
            contentText: spec.afterText,
            // Inherit the user's theme so the dim text keeps a legible contrast
            // ratio in light, dark, and high-contrast themes (never hardcoded).
            color: new vscode.ThemeColor("editorCodeLens.foreground"),
            margin: "0 0 0 1.5rem",
          },
        },
      });
    }
    return options;
  }

  /** Re-render all visible editors (e.g. after a settings or state change). */
  renderVisibleEditors(): void {
    for (const editor of vscode.window.visibleTextEditors) {
      this.renderEditor(editor);
    }
  }

  private clear(editor: vscode.TextEditor): void {
    editor.setDecorations(this.contributionType, []);
  }

  // Invoked by VS Code when the controller (pushed to context.subscriptions in
  // extension.ts) is disposed on deactivate. fallow cannot see that runtime
  // Disposable-contract call because the object is registered directly rather
  // than via an explicit `.dispose()` wrapper.
  // fallow-ignore-next-line unused-class-member
  dispose(): void {
    this.contributionType.dispose();
    this.changeEmitter.dispose();
  }
}
