import * as child_process from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  getLspPath,
  getProduction,
  getDuplicationMinOccurrences,
  getDuplicationMode,
  getDuplicationThreshold,
  getIssueTypes,
  getChangedSince,
  getResolvedConfigPath,
  getAutoDownload,
} from "./config.js";
import { buildAnalysisArgs, countCheckIssues, planDegradation } from "./analysis-utils.js";
import { showBinarySkewToastOnce } from "./binary-skew.js";
import { findBinaryInPath, findLocalBinary, getExecutableExtension } from "./binary-utils.js";
import { downloadCliBinary, getBinaryVersion, getInstalledCliPath } from "./download.js";
import { buildFixArgs, createFixPreviewItems, resolveFixLocation } from "./fix-utils.js";
import type {
  FallowCheckResult,
  FallowCombinedResult,
  FallowDupesResult,
  FallowFixResult,
  FixAction,
} from "./types.js";

export const findCliBinary = (context: vscode.ExtensionContext): string | null => {
  const lspPath = getLspPath();
  if (lspPath) {
    const dir = path.dirname(lspPath);
    const cliPath = path.join(dir, `fallow${getExecutableExtension()}`);
    if (fs.existsSync(cliPath)) {
      return cliPath;
    }
  }

  const local = findLocalBinary("fallow");
  if (local) {
    return local;
  }

  const inPath = findBinaryInPath("fallow");
  if (inPath) {
    return inPath;
  }

  const installed = getInstalledCliPath(context);
  if (installed) {
    return installed;
  }

  return null;
};

export const resolveCliBinary = async (
  context: vscode.ExtensionContext,
): Promise<string | null> => {
  const existing = findCliBinary(context);
  if (existing) {
    return existing;
  }

  if (!getAutoDownload()) {
    return null;
  }

  return downloadCliBinary(context);
};

const execFallow = async (
  context: vscode.ExtensionContext,
  args: ReadonlyArray<string>,
  cwd: string,
): Promise<string> => {
  const binary = await resolveCliBinary(context);

  return await new Promise((resolve, reject) => {
    if (!binary) {
      reject(
        new Error(
          "fallow CLI binary not found. Checked fallow.lspPath sibling, local node_modules/.bin, PATH, managed extension storage, and auto-download.",
        ),
      );
      return;
    }

    const child = child_process.spawn(binary, [...args], {
      cwd,
      stdio: ["ignore", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";

    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      stdout += chunk;
    });

    child.stderr?.setEncoding("utf8");
    child.stderr?.on("data", (chunk: string) => {
      stderr += chunk;
    });

    child.on("error", (error) => {
      reject(error);
    });

    child.on("close", (code, signal) => {
      if (signal) {
        reject(new Error(`fallow exited via signal ${signal}`));
        return;
      }

      if (code !== null && code !== 0 && code !== 1) {
        reject(new Error(stderr.trim() || `fallow exited with code ${code}`));
        return;
      }

      resolve(stdout);
    });
  });
};

/**
 * Resolved CLI versions keyed by binary path. A binary at a given path does not
 * change version within a session, so probe `--version` once instead of on
 * every sidebar analysis (config-change reanalysis can fire these frequently).
 * `undefined` = not yet probed; `null` = probed but version could not be read.
 */
const cliVersionCache = new Map<string, string | null>();

const probeCliVersion = (binaryPath: string): string | null => {
  const cached = cliVersionCache.get(binaryPath);
  if (cached !== undefined) {
    return cached;
  }
  const version = getBinaryVersion(binaryPath);
  cliVersionCache.set(binaryPath, version);
  return version;
};

/**
 * Record that the resolved CLI is older than the extension for some option.
 * Logs the specifics to the output channel on every occurrence (auditable), and
 * surfaces a single actionable toast per session.
 */
const noteBinarySkew = (
  detail: string,
  binaryPath: string | null,
  outputChannel?: vscode.OutputChannel,
): void => {
  outputChannel?.appendLine(`Fallow: ${detail}`);

  const where = binaryPath ? ` (resolved binary: ${binaryPath})` : "";
  showBinarySkewToastOnce(
    `Fallow: the resolved CLI is older than the extension, so some options were ignored and results use CLI defaults for them${where}. Update the fallow binary, or remove the older one from PATH to use the managed auto-download. See the Fallow output channel for details.`,
  );
};

/**
 * Run the analysis, tolerating an older resolved CLI that rejects a flag the
 * extension emits. Version-gated flags are normally omitted up front (see
 * `buildAnalysisArgs`); this is the backstop for when the CLI version could not
 * be probed. On a clap "unexpected argument" naming a known version-gated flag,
 * the flag is stripped and the run retried; every other failure propagates
 * untouched so genuine errors stay loud.
 */
const execAnalysisTolerant = async (
  context: vscode.ExtensionContext,
  initialArgs: ReadonlyArray<string>,
  cwd: string,
  binaryPath: string | null,
  outputChannel?: vscode.OutputChannel,
): Promise<string> => {
  let args: string[] = [...initialArgs];

  for (;;) {
    try {
      return await execFallow(context, args, cwd);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      const plan = planDegradation(message, args);
      if (plan.kind === "rethrow") {
        throw err;
      }

      noteBinarySkew(
        `dropped ${plan.dropped} after the resolved CLI rejected it; this run uses the CLI default for it.`,
        binaryPath,
        outputChannel,
      );
      args = plan.args;
    }
  }
};

/** Filter check results based on the user's issueTypes configuration. */
const filterCheckResult = (result: FallowCheckResult): FallowCheckResult => {
  const types = getIssueTypes();
  const filtered: FallowCheckResult = {
    ...result,
    unused_files: types["unused-files"] ? result.unused_files : [],
    unused_exports: types["unused-exports"] ? result.unused_exports : [],
    unused_types: types["unused-types"] ? result.unused_types : [],
    private_type_leaks: types["private-type-leaks"] ? result.private_type_leaks : [],
    unused_dependencies: types["unused-dependencies"] ? result.unused_dependencies : [],
    unused_dev_dependencies: types["unused-dev-dependencies"] ? result.unused_dev_dependencies : [],
    unused_optional_dependencies: types["unused-optional-dependencies"]
      ? result.unused_optional_dependencies
      : [],
    unused_enum_members: types["unused-enum-members"] ? result.unused_enum_members : [],
    unused_class_members: types["unused-class-members"] ? result.unused_class_members : [],
    unresolved_imports: types["unresolved-imports"] ? result.unresolved_imports : [],
    unlisted_dependencies: types["unlisted-dependencies"] ? result.unlisted_dependencies : [],
    duplicate_exports: types["duplicate-exports"] ? result.duplicate_exports : [],
    type_only_dependencies: types["type-only-dependencies"] ? result.type_only_dependencies : [],
    test_only_dependencies: types["test-only-dependencies"] ? result.test_only_dependencies : [],
    circular_dependencies: types["circular-dependencies"] ? result.circular_dependencies : [],
    re_export_cycles: types["re-export-cycles"] ? result.re_export_cycles : [],
    boundary_violations: types["boundary-violation"] ? result.boundary_violations : [],
    stale_suppressions: types["stale-suppressions"] ? result.stale_suppressions : [],
    unused_catalog_entries: types["unused-catalog-entries"] ? result.unused_catalog_entries : [],
    unresolved_catalog_references: types["unresolved-catalog-references"]
      ? result.unresolved_catalog_references
      : [],
    unused_dependency_overrides: types["unused-dependency-overrides"]
      ? result.unused_dependency_overrides
      : [],
    misconfigured_dependency_overrides: types["misconfigured-dependency-overrides"]
      ? result.misconfigured_dependency_overrides
      : [],
  };
  const totalIssues = countCheckIssues(filtered);
  const summary = {
    total_issues: totalIssues,
    unused_files: filtered.unused_files.length,
    unused_exports: filtered.unused_exports.length,
    unused_types: filtered.unused_types.length,
    private_type_leaks: filtered.private_type_leaks?.length ?? 0,
    unused_dependencies:
      filtered.unused_dependencies.length +
      filtered.unused_dev_dependencies.length +
      (filtered.unused_optional_dependencies?.length ?? 0),
    unused_enum_members: filtered.unused_enum_members.length,
    unused_class_members: filtered.unused_class_members.length,
    unresolved_imports: filtered.unresolved_imports.length,
    unlisted_dependencies: filtered.unlisted_dependencies.length,
    duplicate_exports: filtered.duplicate_exports.length,
    type_only_dependencies: filtered.type_only_dependencies?.length ?? 0,
    test_only_dependencies: filtered.test_only_dependencies?.length ?? 0,
    circular_dependencies: filtered.circular_dependencies?.length ?? 0,
    re_export_cycles: filtered.re_export_cycles?.length ?? 0,
    boundary_violations: filtered.boundary_violations?.length ?? 0,
    stale_suppressions: filtered.stale_suppressions?.length ?? 0,
    unused_catalog_entries: filtered.unused_catalog_entries?.length ?? 0,
    empty_catalog_groups: filtered.empty_catalog_groups?.length ?? 0,
    unresolved_catalog_references: filtered.unresolved_catalog_references?.length ?? 0,
    unused_dependency_overrides: filtered.unused_dependency_overrides?.length ?? 0,
    misconfigured_dependency_overrides: filtered.misconfigured_dependency_overrides?.length ?? 0,
  };
  return {
    ...filtered,
    total_issues: totalIssues,
    summary,
  };
};

const getWorkspaceRoot = (): string | null => {
  const folders = vscode.workspace.workspaceFolders;
  if (!folders || folders.length === 0) {
    return null;
  }
  return folders[0].uri.fsPath;
};

interface FixQuickPickItem extends vscode.QuickPickItem {
  readonly action: "navigate" | "apply-all";
  readonly fix?: FixAction;
}

const confirmApplyFixes = async (): Promise<boolean> => {
  const confirm = await vscode.window.showWarningMessage(
    "Fallow: This will unexport unused exports (keeps the code) and remove unused dependencies from package.json. Continue?",
    "Yes",
    "No",
  );

  return confirm === "Yes";
};

const openFixLocation = async (root: string, fix: FixAction | undefined): Promise<void> => {
  if (!fix) {
    return;
  }

  const location = resolveFixLocation(root, fix);
  if (!location) {
    return;
  }

  await vscode.window.showTextDocument(vscode.Uri.file(location.absolutePath), {
    selection: new vscode.Range(location.line, 0, location.line, 0),
  });
};

const showDryRunPreview = async (root: string, result: FallowFixResult): Promise<void> => {
  if (result.fixes.length === 0) {
    void vscode.window.showInformationMessage("Fallow: no fixes available.");
    return;
  }

  const quickPickItems: FixQuickPickItem[] = [];
  for (const item of createFixPreviewItems(result.fixes)) {
    if (item.action === "apply-all") {
      quickPickItems.push({
        label: "",
        kind: vscode.QuickPickItemKind.Separator,
        action: "navigate",
      });
      quickPickItems.push({
        label: "$(play) Apply all fixes",
        description: item.description,
        action: item.action,
      });
      continue;
    }

    quickPickItems.push({
      label: `$(wrench) ${item.label}`,
      description: item.description,
      detail: item.detail,
      action: item.action,
      fix: item.fix,
    });
  }

  const picked = await vscode.window.showQuickPick(quickPickItems, {
    title: `Fallow: ${result.fixes.length} fix${result.fixes.length === 1 ? "" : "es"} available`,
    placeHolder: "Review fixes. Select 'Apply all fixes' to apply, or click a fix to navigate",
  });

  if (!picked) {
    return;
  }

  if (picked.action === "apply-all") {
    void vscode.commands.executeCommand("fallow.fix");
    return;
  }

  await openFixLocation(root, picked.fix);
};

export const runAnalysis = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<{
  check: FallowCheckResult | null;
  dupes: FallowDupesResult | null;
}> => {
  const root = getWorkspaceRoot();
  if (!root) {
    void vscode.window.showWarningMessage("Fallow: no workspace folder open.");
    return { check: null, dupes: null };
  }

  let check: FallowCheckResult | null = null;
  let dupes: FallowDupesResult | null = null;

  try {
    // Probe the resolved CLI once (no download: findCliBinary, not
    // resolveCliBinary) so version-gated flags can be omitted up front rather
    // than spawn-failed. A null version means "unknown"; we forward
    // optimistically and lean on execAnalysisTolerant as the backstop.
    const cliBinary = findCliBinary(context);
    const cliVersion = cliBinary ? probeCliVersion(cliBinary) : null;

    const { args: analysisArgs, skipped } = buildAnalysisArgs({
      production: getProduction(),
      changedSince: getChangedSince(),
      configPath: getResolvedConfigPath(),
      dupesMode: getDuplicationMode(),
      dupesThreshold: getDuplicationThreshold(),
      minOccurrences: getDuplicationMinOccurrences(),
      cliVersion,
    });

    for (const skip of skipped) {
      noteBinarySkew(
        `omitted ${skip.flag} (your setting is not applied): resolved CLI v${skip.cliVersion} predates v${skip.requires}.`,
        cliBinary,
        outputChannel,
      );
    }

    const output = await execAnalysisTolerant(
      context,
      analysisArgs,
      root,
      cliBinary,
      outputChannel,
    );

    if (output.trim().length === 0) {
      // execFallow already rejects on non-zero exit codes (other than 0/1);
      // an empty stdout on a successful exit means there was nothing to
      // report. Leave check/dupes null and return without raising.
      return { check, dupes };
    }

    const result = JSON.parse(output) as FallowCombinedResult;
    check = result.check ? filterCheckResult(result.check) : null;
    dupes = result.dupes ?? null;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    void vscode.window.showErrorMessage(`Fallow analysis failed: ${message}`);
    throw err;
  }

  return { check, dupes };
};

export const runFix = async (
  context: vscode.ExtensionContext,
  dryRun: boolean,
): Promise<FallowFixResult | null> => {
  const root = getWorkspaceRoot();
  if (!root) {
    void vscode.window.showWarningMessage("Fallow: no workspace folder open.");
    return null;
  }

  if (!dryRun && !(await confirmApplyFixes())) {
    return null;
  }

  try {
    const fixArgs = buildFixArgs(dryRun, getProduction());
    const configPath = getResolvedConfigPath();
    if (configPath) {
      fixArgs.push("--config", configPath);
    }

    const output = await execFallow(context, fixArgs, root);
    const result = JSON.parse(output) as FallowFixResult;

    if (dryRun) {
      await showDryRunPreview(root, result);
    } else {
      const fixCount = result.fixes.length;
      void vscode.window.showInformationMessage(
        `Fallow: applied ${fixCount} fix${fixCount === 1 ? "" : "es"}.`,
      );
    }

    return result;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    void vscode.window.showErrorMessage(`Fallow fix failed: ${message}`);
    return null;
  }
};
