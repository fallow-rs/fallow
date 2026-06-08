// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { compareVersions } from "./analysis-utils.js";
import { execFallow, FallowExecError, resolveCliForRun } from "./commands.js";
import {
  getCoveragePath,
  getCoverageTop,
  getProductionOverride,
  getResolvedConfigPath,
} from "./config.js";
import {
  buildCoverageArgs,
  buildCoverageGateMessage,
  COVERAGE_ANALYZE_MIN_VERSION,
} from "./coverage-utils.js";
import type { CoverageAnalyzeOutput, RuntimeCoverageReport } from "./types.js";

/** Workspace-scoped key persisting the user's chosen capture path. */
const CAPTURE_PATH_SETTING = "coverage.capturePath";

const getWorkspaceRoot = (): string | null => {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    return null;
  }
  return folders[0].uri.fsPath;
};

/**
 * Prompt for a runtime-coverage capture (file or folder) and persist the choice
 * to `fallow.coverage.capturePath` (workspace scope). Returns the absolute path,
 * or null when the user cancels.
 */
const promptForCapturePath = async (): Promise<string | null> => {
  const picked = await vscode.window.showOpenDialog({
    canSelectFiles: true,
    canSelectFolders: true,
    canSelectMany: false,
    openLabel: "Use as Runtime Coverage Capture",
    title: "Select a local runtime-coverage capture (file or folder)",
  });

  const chosen = picked?.[0];
  if (!chosen) {
    return null;
  }

  await vscode.workspace
    .getConfiguration("fallow")
    .update(CAPTURE_PATH_SETTING, chosen.fsPath, vscode.ConfigurationTarget.Workspace);

  return chosen.fsPath;
};

/** Narrow a parsed CLI JSON envelope to the structured-error shape. */
const isStructuredError = (value: unknown): value is { error: true; message?: string } =>
  typeof value === "object" &&
  value !== null &&
  "error" in value &&
  (value as { error: unknown }).error === true;

/**
 * Run `fallow coverage analyze --runtime-coverage <path> --format json` against
 * a local capture and return its `runtime_coverage` block. Mirrors `runAnalysis`
 * in `commands.ts`: resolves (and self-heals) the CLI, version-gates the
 * subcommand, spawns, and surfaces failures as error toasts. Returns null when
 * no workspace is open, no capture path is set/picked, or the run fails.
 *
 * This is fully decoupled from the always-on sidebar pipeline (#902): it runs
 * only on explicit invocation, never during activation or on the LSP path.
 */
export const runCoverageAnalysis = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<RuntimeCoverageReport | null> => {
  const root = getWorkspaceRoot();
  if (!root) {
    void vscode.window.showWarningMessage("Fallow: no workspace folder open.");
    return null;
  }

  const capturePath = getCoveragePath() || (await promptForCapturePath());
  if (!capturePath) {
    return null;
  }

  try {
    const { binary, version } = await resolveCliForRun(context, outputChannel);

    if (version !== null && compareVersions(version, COVERAGE_ANALYZE_MIN_VERSION) < 0) {
      void vscode.window.showErrorMessage(
        `Fallow: runtime coverage requires CLI v${COVERAGE_ANALYZE_MIN_VERSION} or newer (resolved v${version}). Update the fallow binary or enable auto-download.`,
      );
      return null;
    }

    const args = buildCoverageArgs({
      capturePath,
      production: getProductionOverride(),
      top: getCoverageTop(),
      configPath: getResolvedConfigPath(),
    });

    const output = await execFallow(binary, args, root);
    if (output.trim().length === 0) {
      void vscode.window.showWarningMessage(
        "Fallow: the coverage capture produced no runtime data.",
      );
      return null;
    }

    const parsed: unknown = JSON.parse(output);
    if (isStructuredError(parsed)) {
      void vscode.window.showErrorMessage(
        `Fallow coverage failed: ${parsed.message ?? "the capture could not be analyzed."}`,
      );
      return null;
    }

    const result = parsed as CoverageAnalyzeOutput;
    if (!result.runtime_coverage) {
      void vscode.window.showWarningMessage(
        "Fallow: the coverage capture produced no runtime data.",
      );
      return null;
    }

    return result.runtime_coverage;
  } catch (err) {
    // A non-zero gate exit (license 3 / sidecar 4-5) rejects with a
    // FallowExecError that still carries the CLI's structured stdout envelope,
    // which the generic fallback would otherwise discard. Recover the actionable
    // message and the concrete next step (`fallow license activate` / `fallow
    // coverage setup`) for this paid, separately-installed feature.
    if (err instanceof FallowExecError) {
      const message = buildCoverageGateMessage(err.exitCode, err.stdout, err.message);
      void vscode.window.showErrorMessage(`Fallow coverage failed: ${message}`);
      return null;
    }
    const message = err instanceof Error ? err.message : String(err);
    void vscode.window.showErrorMessage(`Fallow coverage failed: ${message}`);
    return null;
  }
};
