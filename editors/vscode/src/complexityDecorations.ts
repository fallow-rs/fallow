import * as path from "node:path";
import * as vscode from "vscode";
import type { ComplexityContribution, ComplexityContributionKind, HealthFinding } from "./types.js";
import { resolveFilePath } from "./treeView-utils.js";

/**
 * Inline "why is this complex" editor decorations.
 *
 * The per-decision-point breakdown arrives on each complexity finding as a
 * `contributions[]` array (one entry per increment event, cyclomatic and
 * cognitive recorded separately). The wire shape is per-increment, but the
 * editor renders per-LINE: contributions are grouped by line and summed into a
 * single dim token, with the per-kind list deferred to a hover. This keeps a
 * dense `if` (one cyclomatic + one cognitive entry on the same line) from
 * stacking two markers.
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

/**
 * A single decoration to place on a document, described by its target line and
 * the content to render. The controller turns this into a `vscode.DecorationOptions`
 * anchored at the END of the line (so the `afterText` never shifts the code) once
 * it has the document to read line lengths from.
 */
export interface ComplexityDecorationSpec {
  /** 0-based line the decoration attaches to. */
  readonly line: number;
  /** Dim end-of-line token (`+N kind`); omitted when the inline tier is off. */
  readonly afterText?: string;
  /** Hover detail. */
  readonly hover: vscode.MarkdownString;
}

/** The two decoration layers produced for one editor. */
export interface ComplexityDecorationGroups {
  /** One per function signature line: aggregate metrics + CRAP explanation. */
  readonly functions: ComplexityDecorationSpec[];
  /** One per contributing line: a summed `+N kind` token + per-kind hover. */
  readonly contributions: ComplexityDecorationSpec[];
}

/**
 * Build the inline decoration specs for one open document from the cached health
 * findings. Pure: no VS Code editor I/O, so it is directly unit-testable. The
 * controller anchors each spec at end-of-line.
 *
 * @param afterText when false, the inline `+N` token is omitted and only the
 *   hover is attached (lets a user keep the quiet hover tier without the dense
 *   per-line text).
 */
export const buildComplexityDecorations = (
  findings: readonly HealthFinding[],
  documentPath: string,
  workspaceRoot: string | undefined,
  options: { readonly afterText: boolean },
): ComplexityDecorationGroups => {
  const matched = findings.filter((f) => sameFile(f.path, documentPath, workspaceRoot));

  const functions: ComplexityDecorationSpec[] = matched.map((finding) => {
    const crap = finding.crap == null ? "" : ` · CRAP ${roundTo(finding.crap, 1)}`;
    return {
      line: Math.max(0, finding.line - 1),
      hover: functionHover(finding),
      afterText: options.afterText
        ? `cyc ${finding.cyclomatic} · cog ${finding.cognitive}${crap}`
        : undefined,
    };
  });

  const contributions: ComplexityDecorationSpec[] = [];
  for (const aggregate of aggregateByLine(matched).values()) {
    contributions.push({
      line: Math.max(0, aggregate.line - 1),
      hover: lineHover(aggregate),
      afterText: options.afterText ? inlineToken(aggregate) : undefined,
    });
  }

  return { functions, contributions };
};

/**
 * Owns the editor decoration type and drives rendering across the active-editor
 * and document-change lifecycle.
 *
 * Line-drift fail-safe: decorations are anchored to the line numbers from the
 * LAST health run. If the user edits a decorated document before the next run,
 * the markers would point at the wrong lines, so on the first edit the document
 * is marked stale and its decorations are cleared until fresh findings arrive
 * (never best-effort re-anchored: a marker on the wrong branch misleads).
 */
export class ComplexityDecorationController {
  // Two decoration types so a one-liner whose signature line is also a
  // contribution line (e.g. `const f = (a) => a ? 1 : 2`) shows BOTH the
  // function summary and the contribution token: VS Code renders only the last
  // `after` attachment per line PER decoration type.
  private readonly functionType: vscode.TextEditorDecorationType;
  private readonly contributionType: vscode.TextEditorDecorationType;
  private findings: readonly HealthFinding[] = [];
  private readonly staleDocuments = new Set<string>();

  constructor(
    private readonly isEnabled: () => boolean,
    private readonly showAfterText: () => boolean,
    private readonly workspaceRoot: () => string | undefined,
  ) {
    this.functionType = vscode.window.createTextEditorDecorationType({});
    this.contributionType = vscode.window.createTextEditorDecorationType({});
  }

  /** Adopt the findings from a completed health run and re-render. */
  setFindings(findings: readonly HealthFinding[]): void {
    this.findings = findings;
    this.staleDocuments.clear();
    this.renderVisibleEditors();
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
  }

  /** Render (or clear) one editor from the cached findings. */
  renderEditor(editor: vscode.TextEditor | undefined): void {
    if (!editor) {
      return;
    }
    const isStale = this.staleDocuments.has(editor.document.uri.toString());
    if (!this.isEnabled() || isStale || editor.document.uri.scheme !== "file") {
      this.clear(editor);
      return;
    }
    const { functions, contributions } = buildComplexityDecorations(
      this.findings,
      editor.document.uri.fsPath,
      this.workspaceRoot(),
      { afterText: this.showAfterText() },
    );
    editor.setDecorations(this.functionType, this.toOptions(editor.document, functions));
    editor.setDecorations(this.contributionType, this.toOptions(editor.document, contributions));
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
      const decoration: vscode.DecorationOptions = {
        range: new vscode.Range(end, end),
        hoverMessage: spec.hover,
      };
      if (spec.afterText !== undefined) {
        decoration.renderOptions = {
          after: {
            contentText: spec.afterText,
            // Inherit the user's theme so the dim text keeps a legible contrast
            // ratio in light, dark, and high-contrast themes (never hardcoded).
            color: new vscode.ThemeColor("editorCodeLens.foreground"),
            margin: "0 0 0 1.5rem",
          },
        };
      }
      options.push(decoration);
    }
    return options;
  }

  /** Re-render all visible editors (e.g. after a settings change). */
  renderVisibleEditors(): void {
    for (const editor of vscode.window.visibleTextEditors) {
      this.renderEditor(editor);
    }
  }

  private clear(editor: vscode.TextEditor): void {
    editor.setDecorations(this.functionType, []);
    editor.setDecorations(this.contributionType, []);
  }

  dispose(): void {
    this.functionType.dispose();
    this.contributionType.dispose();
  }
}
