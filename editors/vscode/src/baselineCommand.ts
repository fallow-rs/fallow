import { execFile } from "node:child_process";
import { registerChild, unregisterChild } from "./process-registry.js";

export const BASELINE_TAG = "fallow-baseline";

const BASELINE_REF = `refs/tags/${BASELINE_TAG}`;
const MAX_GIT_OUTPUT_BYTES = 64 * 1024;

export interface GitCommandResult {
  readonly code: number;
  readonly stdout: string;
  readonly stderr: string;
}

export type GitRunner = (args: ReadonlyArray<string>, cwd: string) => Promise<GitCommandResult>;

export interface BaselineCommandHost {
  readonly workspaceRoots: () => ReadonlyArray<string>;
  readonly changedSince: () => string;
  readonly confirm: (message: string) => Promise<boolean>;
  readonly showWarning: (message: string, action?: string) => Promise<boolean>;
  readonly showError: (message: string) => Promise<void>;
  readonly showInformation: (message: string) => Promise<void>;
  readonly openChangedSinceSetting: () => Promise<void>;
  readonly updateChangedSince: (value: string) => Promise<void>;
  readonly refreshAnalysis: () => Promise<void>;
  readonly appendOutput: (line: string) => void;
}

export type BaselineCommandOutcome =
  | "configured"
  | "already-configured"
  | "cancelled"
  | "no-workspace"
  | "multi-root"
  | "not-repository"
  | "unborn-head"
  | "tag-conflict"
  | "git-error"
  | "settings-error"
  | "refresh-error";

type BaselineGitState =
  | { readonly kind: "missing"; readonly head: string }
  | { readonly kind: "at-head"; readonly head: string }
  | { readonly kind: "elsewhere"; readonly head: string; readonly tagCommit: string }
  | { readonly kind: "not-repository"; readonly detail: string }
  | { readonly kind: "unborn-head"; readonly detail: string }
  | { readonly kind: "error"; readonly detail: string };

const commandDetail = (result: GitCommandResult): string =>
  result.stderr.trim() || result.stdout.trim() || `git exited with code ${result.code}`;

export const runGitCommand: GitRunner = (args, cwd) =>
  new Promise((resolve, reject) => {
    const child = execFile(
      "git",
      [...args],
      {
        cwd,
        encoding: "utf8",
        maxBuffer: MAX_GIT_OUTPUT_BYTES,
      },
      (error, stdout, stderr) => {
        unregisterChild(child);
        if (!error) {
          resolve({ code: 0, stdout, stderr });
          return;
        }
        if (typeof error.code === "number") {
          resolve({ code: error.code, stdout, stderr });
          return;
        }
        reject(error);
      },
    );
    registerChild(child);
  });

const inspectBaselineGitState = async (
  root: string,
  runGit: GitRunner,
): Promise<BaselineGitState> => {
  try {
    const repository = await runGit(["rev-parse", "--show-toplevel"], root);
    if (repository.code !== 0) {
      return { kind: "not-repository", detail: commandDetail(repository) };
    }

    const head = await runGit(["rev-parse", "--verify", "HEAD"], root);
    if (head.code !== 0) {
      return { kind: "unborn-head", detail: commandDetail(head) };
    }
    const headCommit = head.stdout.trim();

    const tagExists = await runGit(["show-ref", "--verify", "--quiet", BASELINE_REF], root);
    if (tagExists.code === 1) {
      return { kind: "missing", head: headCommit };
    }
    if (tagExists.code !== 0) {
      return { kind: "error", detail: commandDetail(tagExists) };
    }

    const tag = await runGit(["rev-parse", "--verify", `${BASELINE_REF}^{commit}`], root);
    if (tag.code !== 0) {
      return { kind: "error", detail: commandDetail(tag) };
    }
    const tagCommit = tag.stdout.trim();
    return tagCommit === headCommit
      ? { kind: "at-head", head: headCommit }
      : { kind: "elsewhere", head: headCommit, tagCommit };
  } catch (error) {
    return {
      kind: "error",
      detail: error instanceof Error ? error.message : String(error),
    };
  }
};

const confirmationMessage = (createTag: boolean, currentChangedSince: string): string => {
  const actions = createTag
    ? `Create the local lightweight Git tag \`${BASELINE_TAG}\` at HEAD and set \`fallow.changedSince\` to that tag?`
    : `Set \`fallow.changedSince\` to the existing \`${BASELINE_TAG}\` tag at HEAD?`;
  const replacement =
    currentChangedSince && currentChangedSince !== BASELINE_TAG
      ? ` This replaces the current effective value \`${currentChangedSince}\`.`
      : "";
  return `${actions}${replacement} This does not push anything to a remote.`;
};

const resolveWorkspaceRoot = async (
  host: BaselineCommandHost,
): Promise<string | "no-workspace" | "multi-root"> => {
  const roots = host.workspaceRoots();
  if (roots.length === 0) {
    await host.showWarning("Fallow: open a workspace folder before setting a baseline.");
    return "no-workspace";
  }
  if (roots.length > 1) {
    await host.showWarning(
      "Fallow: Set Baseline at HEAD currently supports single-folder workspaces only. `fallow.changedSince` is workspace-wide, so no Git ref or setting was changed.",
    );
    return "multi-root";
  }
  return roots[0];
};

const handleUnavailableState = async (
  host: BaselineCommandHost,
  state: Exclude<BaselineGitState, { readonly kind: "missing" | "at-head" }>,
): Promise<BaselineCommandOutcome> => {
  if (state.kind === "not-repository") {
    host.appendOutput(`Set Baseline at HEAD: ${state.detail}`);
    await host.showWarning("Fallow: the workspace folder is not inside a Git repository.");
    return "not-repository";
  }
  if (state.kind === "unborn-head") {
    host.appendOutput(`Set Baseline at HEAD: ${state.detail}`);
    await host.showWarning(
      "Fallow: the repository has no commit yet. Create the first commit before setting a baseline.",
    );
    return "unborn-head";
  }
  if (state.kind === "error") {
    host.appendOutput(`Set Baseline at HEAD failed: ${state.detail}`);
    await host.showError("Fallow: could not inspect the Git baseline. See the Fallow output.");
    return "git-error";
  }

  host.appendOutput(
    `Set Baseline at HEAD: ${BASELINE_TAG} points to ${state.tagCommit}, HEAD is ${state.head}.`,
  );
  const openSettings = await host.showWarning(
    `Fallow: the local tag \`${BASELINE_TAG}\` already points to another commit. It was not moved.`,
    "Open Setting",
  );
  if (openSettings) {
    await host.openChangedSinceSetting();
  }
  return "tag-conflict";
};

const persistChangedSince = async (
  host: BaselineCommandHost,
  createTag: boolean,
): Promise<"settings-error" | null> => {
  try {
    await host.updateChangedSince(BASELINE_TAG);
    return null;
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    host.appendOutput(`Set Baseline at HEAD setting update failed: ${detail}`);
    const prefix = createTag
      ? `The local tag \`${BASELINE_TAG}\` was created, but`
      : `The local tag \`${BASELINE_TAG}\` already points to HEAD, but`;
    await host.showError(
      `Fallow: ${prefix} the workspace setting could not be updated. Set \`fallow.changedSince\` to \`${BASELINE_TAG}\` manually.`,
    );
    return "settings-error";
  }
};

const refreshBaselineAnalysis = async (
  host: BaselineCommandHost,
): Promise<"refresh-error" | null> => {
  try {
    await host.refreshAnalysis();
    return null;
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    host.appendOutput(`Set Baseline at HEAD refresh failed: ${detail}`);
    await host.showError(
      `Fallow: the baseline is configured, but analysis could not refresh. Run "Fallow: Run Analysis" to apply it.`,
    );
    return "refresh-error";
  }
};

const confirmBaselineChange = async (
  host: BaselineCommandHost,
  createTag: boolean,
  settingChanges: boolean,
  currentChangedSince: string,
): Promise<boolean> => {
  if (!createTag && !settingChanges) {
    return true;
  }
  return host.confirm(confirmationMessage(createTag, currentChangedSince));
};

const createBaselineTag = async (
  host: BaselineCommandHost,
  runGit: GitRunner,
  root: string,
): Promise<"git-error" | null> => {
  const created = await runGit(["tag", "--no-sign", BASELINE_TAG, "HEAD"], root);
  if (created.code === 0) {
    return null;
  }
  host.appendOutput(`Set Baseline at HEAD failed: ${commandDetail(created)}`);
  await host.showError("Fallow: could not create the baseline tag. See the Fallow output.");
  return "git-error";
};

const reportBaselineSuccess = async (
  host: BaselineCommandHost,
  alreadyConfigured: boolean,
): Promise<"configured" | "already-configured"> => {
  await host.showInformation(
    alreadyConfigured
      ? `Fallow: \`${BASELINE_TAG}\` is already the active baseline at HEAD.`
      : `Fallow: baseline set at HEAD. New findings are scoped from \`${BASELINE_TAG}\`.`,
  );
  return alreadyConfigured ? "already-configured" : "configured";
};

const configureBaseline = async (
  host: BaselineCommandHost,
  runGit: GitRunner,
  root: string,
  createTag: boolean,
): Promise<BaselineCommandOutcome> => {
  const currentChangedSince = host.changedSince().trim();
  const settingChanges = currentChangedSince !== BASELINE_TAG;
  const confirmed = await confirmBaselineChange(
    host,
    createTag,
    settingChanges,
    currentChangedSince,
  );
  if (!confirmed) {
    return "cancelled";
  }

  if (createTag) {
    const tagFailure = await createBaselineTag(host, runGit, root);
    if (tagFailure) {
      return tagFailure;
    }
  }

  if (settingChanges) {
    const persistenceFailure = await persistChangedSince(host, createTag);
    if (persistenceFailure) {
      return persistenceFailure;
    }
  }

  const refreshFailure = await refreshBaselineAnalysis(host);
  if (refreshFailure) {
    return refreshFailure;
  }

  return reportBaselineSuccess(host, !createTag && !settingChanges);
};

export const runBaselineCommand = async (
  host: BaselineCommandHost,
  runGit: GitRunner = runGitCommand,
): Promise<BaselineCommandOutcome> => {
  const root = await resolveWorkspaceRoot(host);
  if (root === "no-workspace" || root === "multi-root") {
    return root;
  }
  const state = await inspectBaselineGitState(root, runGit);
  if (state.kind !== "missing" && state.kind !== "at-head") {
    return handleUnavailableState(host, state);
  }

  return configureBaseline(host, runGit, root, state.kind === "missing");
};
