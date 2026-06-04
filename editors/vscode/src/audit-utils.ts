import type { AuditGate, AuditOutput, AuditVerdict } from "./types.js";

interface AuditArgsOptions {
  readonly production: boolean;
  readonly changedSince: string;
  readonly configPath: string;
  readonly gate: AuditGate;
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
      audit.summary.duplication_clone_groups
    );
  }
  return (
    audit.attribution.dead_code_introduced +
    audit.attribution.complexity_introduced +
    audit.attribution.duplication_introduced
  );
};

/** Header line framing audit output as static candidates pending verification (#903). */
export const AUDIT_CANDIDATE_HEADER =
  "Audit verdict for your current changes (static candidates, verify before acting).";

const normalizeInlineText = (value: string): string => value.replace(/\s+/g, " ").trim();

const escapeMarkdownText = (value: string): string =>
  normalizeInlineText(value).replace(/([\\`*_{}[\]()#+.!|>-])/g, "\\$1");

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
      label: "dead-code candidates",
    },
    {
      count: all ? audit.summary.complexity_findings : audit.attribution.complexity_introduced,
      icon: "$(pulse)",
      label: "complexity candidates",
    },
    {
      count: all
        ? audit.summary.duplication_clone_groups
        : audit.attribution.duplication_introduced,
      icon: "$(copy)",
      label: "duplication candidates",
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
 * and are kept verbatim. The `base_ref` is markdown-escaped because it can be a
 * user-supplied ref containing markdown metacharacters.
 */
export const buildAuditTooltipMarkdown = (
  audit: AuditOutput,
  changedSinceRef: string | null = null,
): string => {
  const presentation = auditVerdictPresentation(audit.verdict);
  const count = gatingCount(audit);
  const lines: string[] = [`**Fallow Audit** - ${AUDIT_CANDIDATE_HEADER}\n`];

  const baseRef = escapeMarkdownText(audit.base_ref);
  const fileWord = audit.changed_files_count === 1 ? "file" : "files";
  lines.push(`$(git-branch) ${audit.changed_files_count} changed ${fileWord} vs ${baseRef}`);

  if (changedSinceRef) {
    lines.push(`$(history) Scoped to changes since ${escapeMarkdownText(changedSinceRef)}`);
  }

  lines.push(
    `${presentation.icon} Verdict: ${presentation.label}${count > 0 ? ` (${count} gating ${count === 1 ? "candidate" : "candidates"})` : ""}`,
  );

  for (const row of gatingRows(audit)) {
    if (row.count > 0) {
      lines.push(`${row.icon} ${row.count} ${row.label}`);
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
