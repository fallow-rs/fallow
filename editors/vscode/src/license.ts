import * as child_process from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
// VS Code injects this module into the extension host at runtime.
// fallow-ignore-next-line unlisted-dependency
import * as vscode from "vscode";
import { resolveCliForRun } from "./commands.js";
import { getLicenseShowStatusBar } from "./config.js";
import {
  hasLicenseMaterial,
  isValidEmail,
  isValidJwtShape,
  parseLicenseJson,
  validateEmail,
  validateJwtShape,
} from "./license-utils.js";
import { registerChild, unregisterChild } from "./process-registry.js";
import { getStatusBarLicense, setStatusBarLicense } from "./statusBar.js";
import type { LicenseActionResult, LicenseStatusJson } from "./license-types.js";

/**
 * Exit codes the license CLI uses to report a parseable *status* rather than a
 * hard failure. `0` = active/grace, `3` = missing/hard-fail (still emits a
 * valid status envelope). Any other code (2 input/key, 7 network) is a genuine
 * failure whose JSON error envelope we surface.
 */
const STATUS_EXIT_CODES: ReadonlySet<number> = new Set([0, 3]);

interface LicenseExec {
  readonly code: number | null;
  readonly stdout: string;
  readonly stderr: string;
}

/**
 * Spawn `fallow license ...` and collect stdout/stderr. License output is
 * tiny, so the default pipe buffer is fine (no 50MB maxBuffer needed). When
 * `stdin` is provided it is written to the child and the stream closed, so a
 * pasted token reaches the CLI via `--stdin` and never appears in argv.
 */
const execLicense = (
  binary: string | null,
  args: ReadonlyArray<string>,
  cwd: string,
  stdin?: string,
): Promise<LicenseExec> =>
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
      stdio: [stdin === undefined ? "ignore" : "pipe", "pipe", "pipe"],
    });
    registerChild(child);

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
      unregisterChild(child);
      reject(error);
    });

    child.on("close", (code, signal) => {
      unregisterChild(child);
      if (signal) {
        reject(new Error(`fallow exited via signal ${signal}`));
        return;
      }
      resolve({ code, stdout, stderr });
    });

    if (stdin !== undefined && child.stdin) {
      // A child that exits before consuming stdin (stale binary path, a CLI
      // that rejects --stdin) destroys the pipe; without a listener the
      // resulting EPIPE is an uncaught stream error in the extension host. The
      // meaningful failure already surfaces via the child 'error'/'close'
      // handlers above, so the stream error is swallowed, not rerouted.
      child.stdin.once("error", () => {});
      child.stdin.end(stdin);
    }
  });

/** Workspace root when one is open, else the home directory (license is global). */
const licenseCwd = (): string => vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? os.homedir();

/**
 * Run a `fallow license <sub> --format json` invocation and normalize the
 * outcome into a {@link LicenseActionResult}. `{0,3}` are parsed as a status;
 * any other exit code surfaces the structured-error message (falling back to
 * stderr). Spawn failures (missing binary, signal) reject and are caught by the
 * caller.
 */
const runLicense = async (
  context: vscode.ExtensionContext,
  subArgs: ReadonlyArray<string>,
  outputChannel: vscode.OutputChannel | undefined,
  stdin?: string,
): Promise<LicenseActionResult> => {
  const { binary } = await resolveCliForRun(context, outputChannel);
  const args = ["license", ...subArgs, "--format", "json"];
  const { code, stdout, stderr } = await execLicense(binary, args, licenseCwd(), stdin);

  const parsed = parseLicenseJson(stdout);

  if (parsed.ok) {
    // Log only the resolved state, never the token.
    outputChannel?.appendLine(`Fallow license: ${parsed.data.state}`);
    const isStatusExit = code === null || STATUS_EXIT_CODES.has(code);
    return {
      ok: isStatusExit,
      status: parsed.data,
      message: parsed.data.message,
    };
  }

  // Non-status exit, or unparseable output: prefer the structured error
  // message, then stderr, then a generic line.
  const message =
    parsed.error.length > 0 ? parsed.error : stderr.trim() || "fallow license failed.";
  outputChannel?.appendLine(`Fallow license error (exit ${code ?? "?"}): ${message}`);
  return { ok: false, status: null, message };
};

// ---------------------------------------------------------------------------
// Status bar
// ---------------------------------------------------------------------------

/** Default license path mirrored from `fallow_license` (`~/.fallow/license.jwt`). */
const defaultLicensePath = (): string => path.join(os.homedir(), ".fallow", "license.jwt");

/**
 * Whether license material exists on this machine, without shelling out to
 * `fallow`. Checks `$FALLOW_LICENSE` / `$FALLOW_LICENSE_PATH` / the default
 * file, matching the Rust loader precedence. Users who never had a license see
 * no license state in the status bar item.
 */
const licenseMaterialPresent = (): boolean =>
  hasLicenseMaterial(
    process.env["FALLOW_LICENSE"],
    process.env["FALLOW_LICENSE_PATH"],
    defaultLicensePath(),
    (filePath) => {
      try {
        return fs.existsSync(filePath);
      } catch {
        return false;
      }
    },
  );

/**
 * Whether the status bar item shows license state: the indicator is enabled
 * (`fallow.license.showStatusBar`) and license material is present. A state of
 * `missing` with material present means that the material is not valid.
 */
const licenseIndicatorEnabled = (): boolean =>
  getLicenseShowStatusBar() && licenseMaterialPresent();

const applyStatus = (status: LicenseStatusJson | null): void => {
  if (!licenseIndicatorEnabled()) {
    setStatusBarLicense({ kind: "hidden" });
    return;
  }
  setStatusBarLicense(status === null ? { kind: "unchecked" } : { kind: "status", status });
};

/**
 * Put the license state into the status bar item before a probe. Returns
 * false when the indicator is off or no license material is present, so the
 * caller can skip the probe.
 */
export const initLicenseIndicator = (): boolean => {
  if (!licenseIndicatorEnabled()) {
    setStatusBarLicense({ kind: "hidden" });
    return false;
  }
  if (getStatusBarLicense().kind === "hidden") {
    setStatusBarLicense({ kind: "unchecked" });
  }
  return true;
};

/**
 * Probe `fallow license status` and update the status bar item. Best-effort: a
 * missing binary or spawn error keeps the current state and is logged, never
 * surfaced as an error toast (the probe is passive). A no-op for free users
 * (no material).
 */
export const refreshLicenseStatus = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<void> => {
  if (!initLicenseIndicator()) {
    return;
  }
  try {
    const result = await runLicense(context, ["status"], outputChannel);
    applyStatus(result.status);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    outputChannel?.appendLine(`Fallow license: status probe failed: ${message}`);
  }
};

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

type ActivateChoice = "paste" | "file" | "trial";

interface ActivateQuickPickItem extends vscode.QuickPickItem {
  readonly choice: ActivateChoice;
}

const ACTIVATE_ITEMS: ReadonlyArray<ActivateQuickPickItem> = [
  {
    label: "$(key) Paste license token",
    description: "Activate with a token you already have",
    choice: "paste",
  },
  {
    label: "$(file) Activate from file",
    description: "Pick a file containing the license token",
    choice: "file",
  },
  {
    label: "$(rocket) Start a 30-day trial",
    description: "Issue a trial license to your email (requires network)",
    choice: "trial",
  },
];

const afterAction = async (
  context: vscode.ExtensionContext,
  outputChannel: vscode.OutputChannel | undefined,
  result: LicenseActionResult,
): Promise<void> => {
  await refreshLicenseStatus(context, outputChannel);
  if (result.ok) {
    void vscode.window.showInformationMessage(`Fallow: ${result.message}`);
  } else {
    void vscode.window.showErrorMessage(`Fallow: ${result.message}`);
  }
};

const activateFromPaste = async (
  context: vscode.ExtensionContext,
  outputChannel: vscode.OutputChannel | undefined,
): Promise<void> => {
  const token = await vscode.window.showInputBox({
    title: "Activate Fallow License",
    prompt: "Paste your license token",
    password: true,
    ignoreFocusOut: true,
    validateInput: (value) => validateJwtShape(value),
  });
  if (token === undefined || !isValidJwtShape(token)) {
    return;
  }
  // Send via stdin (`--stdin`) so the token never appears in argv.
  const result = await runLicense(context, ["activate", "--stdin"], outputChannel, token);
  await afterAction(context, outputChannel, result);
};

const activateFromFile = async (
  context: vscode.ExtensionContext,
  outputChannel: vscode.OutputChannel | undefined,
): Promise<void> => {
  const picked = await vscode.window.showOpenDialog({
    title: "Select a Fallow license file",
    canSelectMany: false,
    openLabel: "Activate",
  });
  if (!picked || picked.length === 0) {
    return;
  }
  const absolute = picked[0].fsPath;
  const result = await runLicense(context, ["activate", "--from-file", absolute], outputChannel);
  await afterAction(context, outputChannel, result);
};

const activateTrial = async (
  context: vscode.ExtensionContext,
  outputChannel: vscode.OutputChannel | undefined,
): Promise<void> => {
  const email = await vscode.window.showInputBox({
    title: "Start a Fallow Trial",
    prompt: "Email for your 30-day trial",
    ignoreFocusOut: true,
    validateInput: (value) => validateEmail(value),
  });
  if (email === undefined || !isValidEmail(email)) {
    return;
  }
  const result = await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: "Fallow: requesting a trial license...",
      cancellable: false,
    },
    () => runLicense(context, ["activate", "--trial", "--email", email.trim()], outputChannel),
  );
  await afterAction(context, outputChannel, result);
};

export const activateLicenseCommand = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<void> => {
  const picked = await vscode.window.showQuickPick(ACTIVATE_ITEMS, {
    title: "Activate Fallow License",
    placeHolder: "How would you like to activate your license?",
  });
  if (!picked) {
    return;
  }
  try {
    if (picked.choice === "paste") {
      await activateFromPaste(context, outputChannel);
    } else if (picked.choice === "file") {
      await activateFromFile(context, outputChannel);
    } else {
      await activateTrial(context, outputChannel);
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    void vscode.window.showErrorMessage(`Fallow: ${message}`);
  }
};

export const licenseStatusCommand = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<void> => {
  try {
    const result = await runLicense(context, ["status"], outputChannel);
    applyStatus(result.status);
    void vscode.window.showInformationMessage(`Fallow: ${result.message}`);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    void vscode.window.showErrorMessage(`Fallow: ${message}`);
  }
};

export const refreshLicenseCommand = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<void> => {
  try {
    const result = await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Notification,
        title: "Fallow: refreshing license...",
        cancellable: false,
      },
      () => runLicense(context, ["refresh"], outputChannel),
    );
    await afterAction(context, outputChannel, result);
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    void vscode.window.showErrorMessage(`Fallow: ${message}`);
  }
};

export const deactivateLicenseCommand = async (
  context: vscode.ExtensionContext,
  outputChannel?: vscode.OutputChannel,
): Promise<void> => {
  const confirm = await vscode.window.showWarningMessage(
    "Fallow: remove the local license? Paid features will stop working until you re-activate.",
    "Remove",
    "Cancel",
  );
  if (confirm !== "Remove") {
    return;
  }
  try {
    const result = await runLicense(context, ["deactivate"], outputChannel);
    await refreshLicenseStatus(context, outputChannel);
    if (result.ok) {
      void vscode.window.showInformationMessage(`Fallow: ${result.message}`);
    } else {
      void vscode.window.showErrorMessage(`Fallow: ${result.message}`);
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    void vscode.window.showErrorMessage(`Fallow: ${message}`);
  }
};
