// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";

/**
 * Shape read from `_meta.type_aware` of an analysis envelope. Only the two
 * fields that describe a degraded pass are needed here; the generated contract
 * owns the rest. Module-local: callers pass whole envelopes, not this type.
 */
interface TypeAwareDegradationMeta {
  readonly executed?: boolean;
  readonly warnings?: readonly string[] | null;
}

interface TypeAwareMetaBlock {
  readonly type_aware?: TypeAwareDegradationMeta | null;
}

/**
 * The combined run carries the dead-code section's provenance under
 * `_meta.check`; a single-section run carries it at the envelope root.
 */
interface TypeAwareDegradationEnvelope {
  readonly _meta?: (TypeAwareMetaBlock & { readonly check?: TypeAwareMetaBlock | null }) | null;
}

/**
 * Warnings from a semantic pass that could not run.
 *
 * The CLI keeps the syntactic findings when the type-aware pass fails, so the
 * analysis succeeds and reports MORE findings than a refined run would, with
 * type-aware switched on. Without this the editor shows that wider set with no
 * indication that the refinement was skipped.
 */
export const typeAwareDegradationWarnings = (
  result: TypeAwareDegradationEnvelope | null | undefined,
): readonly string[] => {
  const meta = result?._meta?.check?.type_aware ?? result?._meta?.type_aware ?? null;
  if (!meta || meta.executed !== false) {
    return [];
  }
  // The dead-code pass and the coupling pass fail for the same reason and each
  // record it, so the merged metadata repeats one sentence.
  return [...new Set((meta.warnings ?? []).filter((warning) => warning.trim().length > 0))];
};

/**
 * Warning state already notified this session. Analysis re-runs on every save,
 * so a failing sidecar would otherwise raise one toast per run; the details
 * still reach the output channel every time. A different reason is a different
 * state and notifies again.
 */
let notifiedState: string | null = null;

/**
 * Log a degraded semantic pass to the output channel and, once per distinct
 * reason, surface it as a single warning toast.
 */
export const noteTypeAwareDegradation = (
  warnings: readonly string[],
  outputChannel?: vscode.OutputChannel,
): void => {
  if (warnings.length === 0) {
    notifiedState = null;
    return;
  }

  for (const warning of warnings) {
    outputChannel?.appendLine(`Fallow: ${warning}`);
  }

  const state = warnings.join("\n");
  if (notifiedState === state) {
    return;
  }
  notifiedState = state;
  void vscode.window.showWarningMessage(
    `Fallow: type-aware analysis did not run, so these results are the wider syntactic set. ${warnings[0]} See the Fallow output channel for details.`,
  );
};

/** Test-only: reset the once-per-state notification guard. */
export const resetTypeAwareDegradationNotice = (): void => {
  notifiedState = null;
};
