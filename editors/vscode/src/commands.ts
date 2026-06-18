import * as child_process from "node:child_process";
import * as fs from "node:fs";
import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import {
  getLspPath,
  getProductionOverride,
  getAuditGate,
  getDuplicationCrossLanguageOverride,
  getDuplicationIgnoreImportsOverride,
  getDuplicationMinLinesOverride,
  getDuplicationMinOccurrencesOverride,
  getDuplicationMinTokensOverride,
  getDuplicationModeOverride,
  getDuplicationSkipLocalOverride,
  getDuplicationThresholdOverride,
  getHealthHotspots,
  getHealthTopFindings,
  getComplexityBreakdownEnabled,
  getComplexityDecorationCap,
  getIssueTypes,
  getChangedSince,
  getResolvedConfigPath,
  getAutoDownload,
} from "./config.js";
import {
  buildAnalysisArgs,
  compareVersions,
  countCheckIssues,
  planDegradation,
} from "./analysis-utils.js";
import {
  AnalysisBackoffBlockedError,
  AnalysisFailureBackoff,
  buildAnalysisBackoffKey,
  buildAnalysisProcessEnv,
} from "./analysisBackoff.js";
import { buildAuditArgs, parseAuditOutput } from "./audit-utils.js";
import { showBinarySkewToastOnce } from "./binary-skew.js";
import { findBinaryInPath, findLocalBinary, getExecutableExtension } from "./binary-utils.js";
import {
  downloadCliBinary,
  getBinaryVersion,
  getExtensionVersion,
  getInstalledCliPath,
} from "./download.js";
import { buildFixArgs, createFixPreviewItems, resolveFixLocation } from "./fix-utils.js";
import { buildHealthArgs, parseUnknownHealthSubcommand } from "./health-utils.js";
import { registerChild, unregisterChild } from "./process-registry.js";
import { buildSecurityArgs, parseUnknownSubcommand } from "./security-utils.js";
import {
  cacheWorkspacesOutput,
  getCachedWorkspacesOutput,
  parseWorkspacesOutput,
  resolveActiveWorkspaceScope,
} from "./workspacePicker.js";
import type {
  AuditOutput,
  FallowCheckResult,
  FallowCombinedResult,
  FallowDupesResult,
  FallowInspectResult,
  FallowFixResult,
  FixAction,
  HealthOutput,
  SecurityOutput,
  WorkspacesOutput,
} from "./types.js";

export const findCliBinary = async (context: vscode.ExtensionContext): Promise<string | null> => {
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

  const installed = await getInstalledCliPath(context);
  if (installed) {
    return installed;
  }

  return null;
};

export const resolveCliBinary = async (
  context: vscode.ExtensionContext,
): Promise<string | null> => {
  const existing = await findCliBinary(context);
  if (existing) {
    return existing;
  }

  if (!getAutoDownload()) {
    return null;
  }

  return downloadCliBinary(context);
};

/**
 * Rejection carried by {@link execFallow} on a non-zero exit (other than 1).
 * Preserves the child's `exitCode` and captured `stdout` so a caller can recover
 * the structured `{error:true,message,exit_code}` JSON envelope the CLI writes to
 * stdout under `--format json` (e.g. the coverage license/sidecar gate, exit
 * 3/4/5). Existing callers that only read `.message` keep working unchanged.
 */
export class FallowExecError extends Error {
  constructor(
    message: string,
    readonly exitCode: number | null,
    readonly stdout: string,
  ) {
    super(message);
    this.name = "FallowExecError";
  }
}

interface ExecFallowOptions {
  readonly env?: Readonly<Record<string, string>>;
}

/**
 * Ceiling on captured `stdout` (and, symmetrically, `stderr`) for a single
 * `execFallow` spawn. The `spawn` rewrite dropped the original `maxBuffer`, so
 * without this the strings grow unbounded; a runaway CLI (or `--format json`
 * over a pathological monorepo) would balloon the extension-host heap until the
 * host crashes. 50MB matches the prior `maxBuffer` and comfortably exceeds the
 * largest real JSON payloads the JSON call sites parse.
 */
const MAX_STDOUT_BYTES = 50 * 1024 * 1024;

export const execFallow = (
  binary: string | null,
  args: ReadonlyArray<string>,
  cwd: string,
  options: ExecFallowOptions = {},
): Promise<string> =>
  new Promise((resolve, reject) => {
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
      env: options.env ? { ...process.env, ...options.env } : process.env,
      stdio: ["ignore", "pipe", "pipe"],
    });
    registerChild(child);

    let stdout = "";
    let stderr = "";
    let stdoutBytes = 0;
    let stderrBytes = 0;
    // Guards a single reject path: chunks can still arrive (and `close` can
    // fire) after we kill the child, so the first overflow wins and the rest
    // are dropped without re-rejecting an already-settled promise.
    let overflowed = false;

    const rejectOnOverflow = (): void => {
      overflowed = true;
      child.kill();
      reject(
        new Error(
          `fallow output exceeded ${MAX_STDOUT_BYTES / (1024 * 1024)} MB and was aborted. Add large generated files to ignorePatterns, or scope the analysis (e.g. --changed-since), then retry.`,
        ),
      );
    };

    child.stdout?.setEncoding("utf8");
    child.stdout?.on("data", (chunk: string) => {
      if (overflowed) {
        return;
      }
      stdoutBytes += Buffer.byteLength(chunk, "utf8");
      if (stdoutBytes > MAX_STDOUT_BYTES) {
        rejectOnOverflow();
        return;
      }
      stdout += chunk;
    });

    child.stderr?.setEncoding("utf8");
    child.stderr?.on("data", (chunk: string) => {
      if (overflowed) {
        return;
      }
      stderrBytes += Buffer.byteLength(chunk, "utf8");
      if (stderrBytes > MAX_STDOUT_BYTES) {
        rejectOnOverflow();
        return;
      }
      stderr += chunk;
    });

    child.on("error", (error) => {
      unregisterChild(child);
      if (overflowed) {
        return;
      }
      reject(error);
    });

    child.on("close", (code, signal) => {
      unregisterChild(child);
      if (overflowed) {
        return;
      }

      if (signal) {
        const sizeLimit = options.env?.FALLOW_MAX_FILE_SIZE;
        const hint = sizeLimit
          ? ` The analysis process used FALLOW_MAX_FILE_SIZE=${sizeLimit}; lower it or add large generated files to ignorePatterns if memory pressure persists.`
          : "";
        reject(new Error(`fallow exited via signal ${signal}.${hint}`));
        return;
      }

      if (code !== null && code !== 0 && code !== 1) {
        // Preserve stdout so the caller can recover the structured JSON error
        // envelope (`{error:true,message,exit_code}`) the CLI writes there under
        // `--format json`; stderr remains the message for plain consumers.
        reject(
          new FallowExecError(stderr.trim() || `fallow exited with code ${code}`, code, stdout),
        );
        return;
      }

      resolve(stdout);
    });
  });

/**
 * Resolved CLI versions keyed by binary path. A binary at a given path does not
 * change version within a session, so probe `--version` once instead of on
 * every sidebar analysis (config-change reanalysis can fire these frequently).
 * `undefined` = not yet probed; `null` = probed but version could not be read.
 */
const cliVersionCache = new Map<string, Promise<string | null>>();

const probeCliVersion = (binaryPath: string): Promise<string | null> => {
  // Cache the in-flight PROMISE, not the resolved value, so concurrent callers
  // share one `--version` spawn instead of each launching its own.
  const cached = cliVersionCache.get(binaryPath);
  if (cached !== undefined) {
    return cached;
  }
  const version = getBinaryVersion(binaryPath);
  cliVersionCache.set(binaryPath, version);
  return version;
};

/**
 * Resolve the fallow CLI to actually run, self-healing when the binary found on
 * PATH or in `node_modules` is older than the extension itself.
 *
 * The extension and the CLI are versioned and distributed independently, so a
 * user can have a stale global `fallow` (npm, Homebrew, cargo) on PATH that
 * predates flags this extension emits, which silently turns settings such as
 * `duplication.minOccurrences` into no-ops. When auto-download is enabled (the
 * default), switch to the managed CLI, which is pinned to the extension version,
 * so those settings take effect. The managed binary is reused from disk when
 * already present (no network) and fetched once otherwise. An equal-or-newer
 * resolved CLI is always used as-is: this never downgrades, and with
 * auto-download off the stale binary is kept so the caller can degrade loudly.
 *
 * Returns the binary to spawn together with its probed version, so the caller
 * forwards exactly the version-gated flags this binary accepts (no probe/spawn
 * skew).
 */
export const resolveCliForRun = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<{ binary: string | null; version: string | null }> => {
  const found = await findCliBinary(context);
  if (!found) {
    const downloaded = await resolveCliBinary(context);
    return { binary: downloaded, version: downloaded ? await probeCliVersion(downloaded) : null };
  }

  const version = await probeCliVersion(found);
  const required = getExtensionVersion();
  const tooOld = required !== null && version !== null && compareVersions(version, required) < 0;

  if (tooOld && getAutoDownload()) {
    const managed =
      (await getInstalledCliPath(context, outputChannel)) ?? (await downloadCliBinary(context));
    if (managed && managed !== found) {
      const managedVersion = await probeCliVersion(managed);
      outputChannel?.appendLine(
        `Fallow: resolved CLI v${version} predates the extension (v${required}); switched to the managed CLI v${managedVersion ?? "unknown"} so your settings apply.`,
      );
      return { binary: managed, version: managedVersion };
    }
  }

  return { binary: found, version };
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
  initialArgs: ReadonlyArray<string>,
  cwd: string,
  binaryPath: string | null,
  outputChannel?: vscode.OutputChannel,
  options: ExecFallowOptions = {},
): Promise<string> => {
  let args: string[] = [...initialArgs];

  for (;;) {
    try {
      return await execFallow(binaryPath, args, cwd, options);
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

export interface RunAnalysisOptions {
  readonly force?: boolean;
  readonly backoff?: AnalysisFailureBackoff;
}

export interface InspectArgsOptions {
  readonly filePath: string;
  readonly production: boolean | undefined;
  readonly workspace: string;
  readonly configPath: string;
}

export const buildInspectArgs = (options: InspectArgsOptions): string[] => {
  const args = ["inspect", "--file", options.filePath, "--format", "json", "--quiet"];

  if (options.workspace) {
    args.push("--workspace", options.workspace);
  }

  if (options.production) {
    args.push("--production");
  }

  if (options.configPath) {
    args.push("--config", options.configPath);
  }

  return args;
};

const analysisBackoff = new AnalysisFailureBackoff();

const showAnalysisPausedMessage = (failures: number, cause: string | null): void => {
  const suffix = cause ? ` Last failure: ${cause}` : "";
  void vscode.window
    .showErrorMessage(
      `Fallow analysis paused after ${failures} failed attempts for this workspace input. Automatic retries are stopped until you run analysis manually.${suffix}`,
      "Retry now",
    )
    .then((choice) => {
      if (choice === "Retry now") {
        void vscode.commands.executeCommand("fallow.analyze");
      }
      return undefined;
    });
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
    policy_violations: types["policy-violation"] ? result.policy_violations : [],
    stale_suppressions: types["stale-suppressions"] ? result.stale_suppressions : [],
    unused_catalog_entries: types["unused-catalog-entries"] ? result.unused_catalog_entries : [],
    // Intentionally ungateable: there is no `empty-catalog-groups` key in
    // IssueTypeConfig, so it is always passed through. Made explicit (rather than
    // relying on the `...result` spread) so a future spread removal does not
    // silently drop the field, and so the count/filter handling stays in step.
    empty_catalog_groups: result.empty_catalog_groups,
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
    policy_violations: filtered.policy_violations?.length ?? 0,
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
  options: RunAnalysisOptions = {},
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
  let backoffKey: string | null = null;
  const backoff = options.backoff ?? analysisBackoff;

  try {
    // Resolve the CLI to run, self-healing to the managed binary when the one
    // found on PATH/node_modules is older than the extension (otherwise its
    // settings would be silent no-ops). The probed version belongs to the
    // binary we will actually spawn, so version-gated flags are forwarded only
    // when accepted; a null version means "unknown" and we forward optimistically
    // and lean on execAnalysisTolerant as the backstop.
    const { binary: cliBinary, version: cliVersion } = await resolveCliForRun(
      context,
      outputChannel,
    );

    const { args: analysisArgs, skipped } = buildAnalysisArgs({
      production: getProductionOverride(),
      changedSince: getChangedSince(),
      workspace: resolveActiveWorkspaceScope(context),
      configPath: getResolvedConfigPath(),
      dupesMode: getDuplicationModeOverride(),
      dupesThreshold: getDuplicationThresholdOverride(),
      dupesMinTokens: getDuplicationMinTokensOverride(),
      dupesMinLines: getDuplicationMinLinesOverride(),
      minOccurrences: getDuplicationMinOccurrencesOverride(),
      dupesSkipLocal: getDuplicationSkipLocalOverride(),
      dupesCrossLanguage: getDuplicationCrossLanguageOverride(),
      dupesIgnoreImports: getDuplicationIgnoreImportsOverride(),
      cliVersion,
    });
    backoffKey = buildAnalysisBackoffKey(root, analysisArgs);
    const blocked = backoff.blockedNotice(backoffKey, options.force === true);
    if (blocked) {
      if (blocked.shouldNotify) {
        showAnalysisPausedMessage(blocked.failures, null);
      }
      throw new AnalysisBackoffBlockedError(blocked.failures);
    }

    for (const skip of skipped) {
      noteBinarySkew(
        `omitted ${skip.flag} (your setting is not applied): resolved CLI v${skip.cliVersion} predates v${skip.requires}.`,
        cliBinary,
        outputChannel,
      );
    }

    const output = await execAnalysisTolerant(analysisArgs, root, cliBinary, outputChannel, {
      env: buildAnalysisProcessEnv(),
    });

    if (output.trim().length === 0) {
      // execFallow already rejects on non-zero exit codes (other than 0/1);
      // an empty stdout on a successful exit means there was nothing to
      // report. Leave check/dupes null and return without raising.
      backoff.recordSuccess(backoffKey);
      return { check, dupes };
    }

    const result = JSON.parse(output) as FallowCombinedResult;
    check = result.check ? filterCheckResult(result.check) : null;
    dupes = result.dupes ?? null;
    backoff.recordSuccess(backoffKey);
  } catch (err) {
    if (err instanceof AnalysisBackoffBlockedError) {
      throw err;
    }
    const message = err instanceof Error ? err.message : String(err);
    const paused =
      backoffKey !== null && options.force !== true ? backoff.recordFailure(backoffKey) : null;
    if (paused) {
      if (paused.shouldNotify) {
        showAnalysisPausedMessage(paused.failures, message);
      }
    } else {
      void vscode.window.showErrorMessage(`Fallow analysis failed: ${message}`);
    }
    throw err;
  }

  return { check, dupes };
};

/**
 * List monorepo workspace packages via `fallow workspaces --format json`,
 * populating the workspace picker. Cached per resolved binary path for the
 * session (the package list is stable within a session); `forceRefresh`
 * busts the cache. On a missing-subcommand failure (an old CLI), shows an
 * actionable toast and returns null so the picker leaves scope unchanged.
 * Not routed through `planDegradation` (its allowlist is analysis flags).
 *
 * `silent` suppresses every user-facing toast (errors still go to the output
 * channel). The background single-package-visibility probe (n2) passes it so a
 * passive scope check never nags about an old CLI or a missing binary; the
 * user-initiated picker path leaves it false so genuine failures stay loud.
 */
export const runWorkspaces = async (
  context: vscode.ExtensionContext,
  forceRefresh: boolean,
  outputChannel?: vscode.OutputChannel,
  silent = false,
): Promise<WorkspacesOutput | null> => {
  const root = getWorkspaceRoot();
  if (!root) {
    if (!silent) {
      void vscode.window.showWarningMessage("Fallow: no workspace folder open.");
    }
    return null;
  }

  const { binary } = await resolveCliForRun(context, outputChannel);
  if (!binary) {
    if (!silent) {
      void vscode.window.showErrorMessage(
        "Fallow: CLI binary not found. Enable fallow.autoDownload or set fallow.lspPath.",
      );
    }
    return null;
  }

  if (!forceRefresh) {
    const cached = getCachedWorkspacesOutput(binary);
    if (cached) {
      return cached;
    }
  }

  try {
    const output = await execFallow(binary, ["workspaces", "--format", "json", "--quiet"], root);
    const parsed = parseWorkspacesOutput(output);
    if (!parsed) {
      outputChannel?.appendLine(
        "Fallow: `fallow workspaces` returned no parseable JSON; workspace scoping unavailable.",
      );
      return null;
    }
    cacheWorkspacesOutput(binary, parsed);
    return parsed;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    outputChannel?.appendLine(`Fallow: failed to list workspaces: ${message}`);
    if (silent) {
      return null;
    }
    if (/unrecognized subcommand|unexpected argument|wasn't expected/i.test(message)) {
      void vscode.window.showWarningMessage(
        "Fallow: this CLI version does not support `fallow workspaces`. Update the fallow binary to scope analysis to a monorepo package.",
      );
    } else {
      void vscode.window.showErrorMessage(`Fallow: failed to list workspaces: ${message}`);
    }
    return null;
  }
};

/**
 * Single-flight guard for the audit run. A second invocation while one is in
 * flight is ignored, preventing overlapping base-snapshot worktree-cache
 * contention when a user spams the command or rapid save-triggers fire.
 */
let auditInFlight = false;

/**
 * Run `fallow audit --format json` against the workspace and parse the verdict
 * envelope. Mirrors `runAnalysis`: resolves the self-healing managed CLI,
 * builds a lean argv (only flags that shipped with the `audit` command, so no
 * version-gated degradation is needed), spawns via `execFallow`, and returns
 * the parsed `AuditOutput`.
 *
 * Returns null when there is no workspace, the run is already in flight, or the
 * output is not a parseable audit envelope. Audit exits 1 on a `fail` verdict,
 * which `execFallow` resolves as success, so a failing verdict still returns a
 * non-null result. On a spawn error the error toast surfaces and the error is
 * rethrown so the caller can flip the status bar to its error state.
 */
export const runAudit = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<AuditOutput | null> => {
  const root = getWorkspaceRoot();
  if (!root) {
    void vscode.window.showWarningMessage("Fallow: no workspace folder open.");
    return null;
  }

  if (auditInFlight) {
    return null;
  }
  auditInFlight = true;

  try {
    const { binary: cliBinary } = await resolveCliForRun(context, outputChannel);
    const auditArgs = buildAuditArgs({
      production: getProductionOverride(),
      changedSince: getChangedSince(),
      workspace: resolveActiveWorkspaceScope(context),
      configPath: getResolvedConfigPath(),
      gate: getAuditGate(),
    });

    const output = await execFallow(cliBinary, auditArgs, root);
    return parseAuditOutput(output);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    void vscode.window.showErrorMessage(`Fallow audit failed: ${message}`);
    throw err;
  } finally {
    auditInFlight = false;
  }
};

const activeEditorInspectTarget = (): {
  readonly document: vscode.TextDocument;
  readonly root: string;
  readonly filePath: string;
} | null => {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    void vscode.window.showWarningMessage("Fallow: no active editor to inspect.");
    return null;
  }

  if (editor.document.uri.scheme !== "file") {
    void vscode.window.showWarningMessage("Fallow: active editor is not a file on disk.");
    return null;
  }

  const folder = vscode.workspace.getWorkspaceFolder(editor.document.uri);
  const root = folder?.uri.fsPath ?? getWorkspaceRoot();
  if (!root) {
    void vscode.window.showWarningMessage("Fallow: no workspace folder open.");
    return null;
  }

  const relative = path.relative(root, editor.document.uri.fsPath);
  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    void vscode.window.showWarningMessage("Fallow: active editor is outside the workspace.");
    return null;
  }

  return {
    document: editor.document,
    root,
    filePath: relative.split(path.sep).join(path.posix.sep),
  };
};

export const runInspectActiveFile = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<FallowInspectResult | null> => {
  const target = activeEditorInspectTarget();
  if (!target) {
    return null;
  }

  try {
    if (target.document.isDirty) {
      const saved = await target.document.save();
      if (!saved) {
        void vscode.window.showWarningMessage(
          `Fallow inspect cancelled because ${target.filePath} could not be saved.`,
        );
        return null;
      }
    }

    const { binary } = await resolveCliForRun(context, outputChannel);
    const args = buildInspectArgs({
      filePath: target.filePath,
      production: getProductionOverride(),
      workspace: resolveActiveWorkspaceScope(context),
      configPath: getResolvedConfigPath(),
    });

    const output = await execFallow(binary, args, target.root);
    if (output.trim().length === 0) {
      void vscode.window.showWarningMessage("Fallow inspect returned no output.");
      return null;
    }

    const result = JSON.parse(output) as FallowInspectResult;
    outputChannel?.appendLine(`Fallow inspect: ${target.filePath}`);
    outputChannel?.appendLine(JSON.stringify(result, null, 2));
    outputChannel?.show();
    void vscode.window.showInformationMessage(`Fallow inspect complete: ${target.filePath}`);
    return result;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    void vscode.window.showErrorMessage(`Fallow inspect failed: ${message}`);
    return null;
  }
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
    const fixArgs = buildFixArgs(dryRun, getProductionOverride());
    const configPath = getResolvedConfigPath();
    if (configPath) {
      fixArgs.push("--config", configPath);
    }

    const { binary } = await resolveCliForRun(context);
    const output = await execFallow(binary, fixArgs, root);
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

/**
 * Whether the per-session "no workspace folder" warning has already been shown
 * for the Health view. The Health view re-spawns on every reveal until it
 * latches a completed run, so without this gate a workspace-less window would
 * repeat the same toast on every re-reveal (#902).
 */
let healthNoWorkspaceWarned = false;

/** Test-only: reset the once-per-session no-workspace gate. */
export const resetHealthNoWorkspaceWarning = (): void => {
  healthNoWorkspaceWarned = false;
};

/**
 * Run a standalone `fallow health` analysis for the Health view. This is a
 * separate spawn from {@link runAnalysis}, fired lazily only when the Health
 * view is first revealed, so it never slows the latency-critical combined run
 * (which keeps `--skip health`). Hotspots (a git-churn walk) are requested only
 * when the user opted in via `fallow.health.hotspots`.
 *
 * Reuses the same binary resolution and spawn primitive as the combined run.
 * `execFallow` already tolerates exit 0/1 (health exits 1 when findings exist)
 * and rejects only on signal or other non-zero codes.
 *
 * Returns null for the non-retryable outcomes (no workspace, empty output, or a
 * resolved CLI that predates `fallow health`), so the caller latches the run as
 * complete and does not re-spawn or re-toast on the next reveal. A genuine
 * transient failure (spawn/parse error) is rethrown so the caller can reset its
 * latch and retry on a later reveal (#902).
 */
export const runHealthAnalysis = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<HealthOutput | null> => {
  const root = getWorkspaceRoot();
  if (!root) {
    // Non-retryable until a folder is opened; warn once so re-reveals stay
    // quiet rather than repeating the toast on every Health-view visibility.
    if (!healthNoWorkspaceWarned) {
      healthNoWorkspaceWarned = true;
      void vscode.window.showWarningMessage("Fallow: no workspace folder open.");
    }
    return null;
  }

  try {
    const { binary } = await resolveCliForRun(context, outputChannel);

    // When the inline breakdown is on, fetch a larger top-N so files outside
    // the tree's top-N still get decorated; the tree slices back to
    // `getHealthTopFindings()` for display.
    const breakdownEnabled = getComplexityBreakdownEnabled();
    const topFindings = breakdownEnabled
      ? Math.max(getHealthTopFindings(), getComplexityDecorationCap())
      : getHealthTopFindings();

    const args = buildHealthArgs({
      hotspots: getHealthHotspots(),
      topFindings,
      configPath: getResolvedConfigPath(),
      changedSince: getChangedSince(),
      production: getProductionOverride(),
      workspace: resolveActiveWorkspaceScope(context),
      complexityBreakdown: breakdownEnabled,
    });

    const output = await execFallow(binary, args, root);

    if (output.trim().length === 0) {
      // A successful exit with empty stdout means there was nothing to report.
      return null;
    }

    return JSON.parse(output) as HealthOutput;
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);

    // An older CLI without the `health` subcommand is a known, non-retryable
    // state (mirrors runSecurityAnalysis): warn once with an actionable
    // message and render an empty view, rather than re-spawning on every
    // reveal or surfacing a raw clap stderr blob as an error.
    if (parseUnknownHealthSubcommand(message)) {
      outputChannel?.appendLine(
        `Fallow: the resolved CLI does not support \`fallow health\`. ${message}`,
      );
      void vscode.window.showWarningMessage(
        "Fallow: update the fallow CLI to analyze project health.",
      );
      return null;
    }

    // Genuine transient failure: surface it and rethrow so the caller resets
    // its latch and a later reveal retries.
    void vscode.window.showErrorMessage(`Fallow health analysis failed: ${message}`);
    throw err;
  }
};

/**
 * Outcome of a `fallow security` scan. The discriminant separates a genuinely
 * completed scan (which may legitimately have zero findings) from a scan that
 * never produced a verdict (no workspace, older CLI, or a transient failure).
 * The caller needs that distinction so it only paints the "No security
 * candidates found" all-clear after a real scan, never after a failure (#903).
 *
 * - `{ ok: true, data: SecurityOutput }`: scan ran, findings present.
 * - `{ ok: true, data: null }`: scan ran, genuinely nothing to report.
 * - `{ ok: false, retryable: false }`: non-retryable (no workspace, older CLI);
 *   the latch should hold so a re-reveal does not re-warn.
 * - `{ ok: false, retryable: true }`: transient failure; the latch should reset
 *   so a later reveal retries.
 */
export type SecurityScanResult =
  | { readonly ok: true; readonly data: SecurityOutput | null }
  | { readonly ok: false; readonly retryable: boolean };

/**
 * Run `fallow security --format json` and parse its `SecurityOutput` envelope.
 * This is a SEPARATE, independent process from the combined sidebar analysis
 * (security findings are `#[serde(skip)]` on `AnalysisResults` and never appear
 * under bare `fallow`), so the dead-code / duplicates sidebar latency path is
 * untouched (#902).
 *
 * Findings are UNVERIFIED candidates, not confirmed vulnerabilities; the caller
 * frames them as such in every surface (#903). A resolved CLI that predates the
 * `security` subcommand degrades to a one-line "update fallow" warning and a
 * non-retryable failure result, rather than surfacing a raw clap stderr blob.
 * Returns a {@link SecurityScanResult} so the caller can tell a clean scan apart
 * from a failed one and avoid painting a false all-clear.
 */
export const runSecurityAnalysis = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<SecurityScanResult> => {
  const root = getWorkspaceRoot();
  if (!root) {
    void vscode.window.showWarningMessage("Fallow: no workspace folder open.");
    return { ok: false, retryable: false };
  }

  try {
    const { binary } = await resolveCliForRun(context, outputChannel);
    const args = buildSecurityArgs({
      configPath: getResolvedConfigPath(),
      changedSince: getChangedSince(),
      workspace: resolveActiveWorkspaceScope(context),
    });

    const output = await execFallow(binary, args, root);
    if (output.trim().length === 0) {
      return { ok: true, data: null };
    }

    return { ok: true, data: JSON.parse(output) as SecurityOutput };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);

    // An older CLI without the `security` subcommand is a known, non-retryable
    // state: warn once with an actionable message and leave the actionable
    // enable/scan welcome in place, rather than surfacing a raw stderr blob or
    // a false "all-clear".
    if (parseUnknownSubcommand(message)) {
      outputChannel?.appendLine(
        `Fallow: the resolved CLI does not support security candidates. ${message}`,
      );
      void vscode.window.showWarningMessage(
        "Fallow: update the fallow CLI to scan for security candidates.",
      );
      return { ok: false, retryable: false };
    }

    void vscode.window.showErrorMessage(`Fallow security scan failed: ${message}`);
    return { ok: false, retryable: true };
  }
};
