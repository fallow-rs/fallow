import { escapeMarkdownText } from "./markdown-utils.js";
import type { AuditGate, AuditOutput, AuditVerdict } from "./types.js";

interface AuditArgsOptions {
  /**
   * `true` forwards `--production`; `false`/`undefined` defer to the project
   * config. Force-off (`--no-production`) is intentionally not wired into the
   * standalone audit run: the editor-parity fix (#1055) scopes force-off to the
   * sidebar + LSP diagnostics surfaces.
   */
  readonly production: boolean | undefined;
  readonly changedSince: string;
  readonly configPath: string;
  readonly gate: AuditGate;
  /**
   * Monorepo workspace scope (a package name). When non-empty, forwarded as
   * `--workspace <name>` so the audit verdict reflects only that package. NOT
   * version-gated: `--workspace` is a long-standing global CLI flag. Mirrors
   * the workspace forwarding in `buildAnalysisArgs`.
   */
  readonly workspace: string;
}

/**
 * Build the argument vector for an on-demand `fallow audit` run. Kept pure (no
 * config / VS Code access) so the flag-forwarding rules can be unit-tested.
 *
 * `audit` is the first positional (the subcommand selector) and must precede
 * every flag. The argv stays lean: only flags that shipped with the `audit`
 * command itself are emitted (`--format`, `--quiet`, `--changed-since`,
 * `--production`, `--config`, `--gate`), so there is nothing version-gated to
 * strip on an older CLI. Audit owns its own sub-pass selection, so no `--skip`
 * is passed. The sidebar's `--dupes-*` tuning knobs are intentionally not
 * forwarded; audit does not accept them in this surface.
 *
 * `--gate all` is appended only when explicitly requested. The CLI default is
 * `new-only`, so omitting the flag for the default keeps the argv minimal and
 * matches the established `buildAnalysisArgs` style (default values are no-ops
 * we simply omit).
 */
export const buildAuditArgs = (options: AuditArgsOptions): string[] => {
  const args = ["audit", "--format", "json", "--quiet"];

  if (options.changedSince) {
    args.push("--changed-since", options.changedSince);
  }

  if (options.workspace) {
    args.push("--workspace", options.workspace);
  }

  if (options.production) {
    args.push("--production");
  }

  if (options.configPath) {
    args.push("--config", options.configPath);
  }

  if (options.gate === "all") {
    args.push("--gate", "all");
  }

  return args;
};

/**
 * The status-bar theme color key for a warn/fail audit verdict, or null for a
 * passing verdict (no background tint). Uses VS Code's built-in status-bar
 * severity theme colors so the surface respects the user's theme rather than
 * hard-coding any color.
 */
export type AuditSeverityKey = "statusBarItem.errorBackground" | "statusBarItem.warningBackground";

export interface AuditVerdictPresentation {
  readonly icon: string;
  readonly label: AuditVerdict;
  readonly background: AuditSeverityKey | null;
}

const stylingGatingCount = (audit: AuditOutput): number => {
  const findings = audit.complexity?.styling_findings ?? [];
  const newOnly = audit.attribution.gate === "new-only";
  return findings.filter(
    (finding) =>
      finding.effective_severity === "error" && (!newOnly || finding.introduced === true),
  ).length;
};

/**
 * Map a verdict to its status-bar icon, label, and (theme-color) background.
 * `pass` carries no background tint; `warn` and `fail` map to the built-in
 * status-bar warning / error theme colors.
 */
export const auditVerdictPresentation = (verdict: AuditVerdict): AuditVerdictPresentation => {
  if (verdict === "fail") {
    return { icon: "$(error)", label: "fail", background: "statusBarItem.errorBackground" };
  }
  if (verdict === "warn") {
    return { icon: "$(warning)", label: "warn", background: "statusBarItem.warningBackground" };
  }
  return { icon: "$(pass)", label: "pass", background: null };
};

/**
 * Count of gating findings that drove the verdict, matched to the active gate.
 *
 * The CLI owns the verdict; this count exists only so the number the user sees
 * in the status bar and tooltip is the number the active gate actually fails
 * on. Under `new-only` the verdict reflects *introduced* findings, so the count
 * sums the `*_introduced` attribution fields (inherited noise that does not
 * flip the verdict is excluded). Under `all` every finding in the changed set
 * is gating, so the count sums the `summary` totals.
 */
export const gatingCount = (audit: AuditOutput): number => {
  if (audit.attribution.gate === "all") {
    return (
      audit.summary.dead_code_issues +
      audit.summary.complexity_findings +
      audit.summary.duplication_clone_groups +
      stylingGatingCount(audit)
    );
  }
  return (
    audit.attribution.dead_code_introduced +
    audit.attribution.complexity_introduced +
    audit.attribution.duplication_introduced +
    stylingGatingCount(audit)
  );
};

/** Header line framing audit output as static candidates pending verification (#903). */
export const AUDIT_CANDIDATE_HEADER =
  "Audit verdict for your current changes (static candidates, verify before acting).";

/**
 * Abbreviate a 40-char hex SHA to 12 chars for display; leave branch names and
 * refspecs untouched. Mirrors the CLI's `short_base_ref` so an auto-detected
 * `merge-base` base shows `611d151e8250` rather than a raw 40-char SHA (#1168).
 */
const shortBaseRef = (baseRef: string): string =>
  /^[0-9a-f]{40}$/.test(baseRef) ? baseRef.slice(0, 12) : baseRef;

/**
 * Display form of the audit base: the abbreviated `base_ref` plus its
 * provenance when present, e.g. `611d151e8250 (merge-base with origin/main)`.
 * `base_description` is absent for an explicit `--base`, so those show the ref
 * the user typed verbatim. `escape` is applied per-component for the
 * trusted-markdown tooltip; the plain-text toast passes it through unchanged.
 */
const formatAuditBase = (audit: AuditOutput, escape: (s: string) => string): string => {
  const baseRef = escape(shortBaseRef(audit.base_ref));
  if (!audit.base_description) {
    return baseRef;
  }
  return `${baseRef} (${escape(audit.base_description)})`;
};

/**
 * Plain-text change-set scope summary for the audit verdict, e.g.
 * `1 changed file vs 611d151e8250 (merge-base with origin/main)`. Used in the
 * disabled-status-bar info toast so the verdict word carries scope context
 * rather than standing alone (#908 n3). Plain text (not markdown), since it
 * goes into a `showInformationMessage`.
 */
export const auditScopeSummary = (audit: AuditOutput): string => {
  const fileWord = audit.changed_files_count === 1 ? "file" : "files";
  const base = formatAuditBase(audit, (s) => s);
  return `${audit.changed_files_count} changed ${fileWord} vs ${base}`;
};

/**
 * The trailing ` (N)` gating-count suffix for the status-bar verdict label.
 * Shown for any non-zero gating count regardless of verdict, so a `warn`
 * verdict's glance matches the tooltip's own `count > 0` branch (a `warn` that
 * suppressed the count read as a clean pass at a glance). Empty when nothing is
 * gating. Pure so the rule is unit-tested without a status-bar mock.
 */
export const auditGatingSuffix = (audit: AuditOutput): string => {
  const count = gatingCount(audit);
  return count > 0 ? ` (${count})` : "";
};

interface GatingRow {
  readonly count: number;
  readonly icon: string;
  readonly label: string;
}

/**
 * Per-category gating breakdown for the active gate. Mirrors `gatingCount`'s
 * source selection so the rows sum to the displayed count.
 */
const gatingRows = (audit: AuditOutput): readonly GatingRow[] => {
  const all = audit.attribution.gate === "all";
  return [
    {
      count: all ? audit.summary.dead_code_issues : audit.attribution.dead_code_introduced,
      icon: "$(circle-slash)",
      label: "dead-code candidate",
    },
    {
      count: all ? audit.summary.complexity_findings : audit.attribution.complexity_introduced,
      icon: "$(pulse)",
      label: "complexity candidate",
    },
    {
      count: all
        ? audit.summary.duplication_clone_groups
        : audit.attribution.duplication_introduced,
      icon: "$(copy)",
      label: "duplication candidate",
    },
    {
      count: stylingGatingCount(audit),
      icon: "$(symbol-color)",
      label: "styling candidate",
    },
  ];
};

/**
 * Trusted-markdown tooltip for the audit status-bar item.
 *
 * Lists the scope (changed-file count vs base ref), the verdict, the per-
 * category gating breakdown (non-zero rows only), and command-link footer.
 * Finding-level wording uses "candidate" framing (#903), never "defects" or
 * "problems"; the verdict words pass/warn/fail are the CLI's own gate language
 * and are kept verbatim. The `base_ref` and `base_description` are
 * markdown-escaped because they can carry user-supplied refs containing
 * markdown metacharacters.
 */
export const buildAuditTooltipMarkdown = (
  audit: AuditOutput,
  changedSinceRef: string | null = null,
): string => {
  const presentation = auditVerdictPresentation(audit.verdict);
  const count = gatingCount(audit);
  const lines: string[] = [`**Fallow Audit** - ${AUDIT_CANDIDATE_HEADER}\n`];

  const base = formatAuditBase(audit, escapeMarkdownText);
  const fileWord = audit.changed_files_count === 1 ? "file" : "files";
  lines.push(`$(git-branch) ${audit.changed_files_count} changed ${fileWord} vs ${base}`);

  if (changedSinceRef) {
    lines.push(`$(history) Scoped to changes since ${escapeMarkdownText(changedSinceRef)}`);
  }

  lines.push(
    `${presentation.icon} Verdict: ${presentation.label}${count > 0 ? ` (${count} gating ${count === 1 ? "candidate" : "candidates"})` : ""}`,
  );

  for (const row of gatingRows(audit)) {
    if (row.count > 0) {
      lines.push(`${row.icon} ${row.count} ${row.label}${row.count === 1 ? "" : "s"}`);
    }
  }

  if (count === 0) {
    lines.push("$(check) No gating candidates in the current change set");
  }

  lines.push("\n---\n");
  lines.push(
    "[$(sync) Re-run](command:fallow.audit) · [$(output) Details](command:fallow.showOutput)",
  );

  return lines.join("\n\n");
};

/**
 * Parse `fallow audit --format json` stdout into a typed `AuditOutput`.
 *
 * Returns null on empty / whitespace stdout (no result to render) and on any
 * payload that is not a real audit envelope: a non-`"audit"` `command`, a
 * missing `verdict`, or a parse error. Audit exits 1 on a `fail` verdict, which
 * `execFallow` treats as success (it resolves stdout for exit codes 0 and 1),
 * so a `fail` verdict still yields parseable stdout and a non-null result here.
 */
export const parseAuditOutput = (stdout: string): AuditOutput | null => {
  if (stdout.trim().length === 0) {
    return null;
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    return null;
  }

  if (typeof parsed !== "object" || parsed === null) {
    return null;
  }

  const candidate = parsed as Partial<AuditOutput>;
  if (candidate.command !== "audit") {
    return null;
  }
  if (
    candidate.verdict !== "pass" &&
    candidate.verdict !== "warn" &&
    candidate.verdict !== "fail"
  ) {
    return null;
  }

  return parsed as AuditOutput;
};
