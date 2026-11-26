const ANALYSIS_FAILURE_LIMIT = 3;
export const ANALYSIS_DEFAULT_MAX_FILE_SIZE_MB = "5";

interface BackoffEntry {
  readonly key: string;
  readonly failures: number;
  readonly paused: boolean;
  readonly notified: boolean;
}

export interface AnalysisBackoffNotice {
  readonly failures: number;
  readonly shouldNotify: boolean;
}

export class AnalysisBackoffBlockedError extends Error {
  constructor(readonly failures: number) {
    super(
      `automatic analysis is paused after ${failures} failed attempts for this workspace input. Run Fallow: Run Analysis to retry.`,
    );
    this.name = "AnalysisBackoffBlockedError";
  }
}

export class AnalysisFailureBackoff {
  private entry: BackoffEntry | null = null;

  constructor(private readonly failureLimit = ANALYSIS_FAILURE_LIMIT) {}

  blockedNotice(key: string, force: boolean): AnalysisBackoffNotice | null {
    if (force) {
      this.reset(key);
      return null;
    }

    if (!this.entry || this.entry.key !== key || !this.entry.paused) {
      return null;
    }

    const shouldNotify = !this.entry.notified;
    this.entry = { ...this.entry, notified: true };
    return { failures: this.entry.failures, shouldNotify };
  }

  recordFailure(key: string): AnalysisBackoffNotice | null {
    const current =
      this.entry && this.entry.key === key
        ? this.entry
        : { key, failures: 0, paused: false, notified: false };
    const failures = current.failures + 1;
    const paused = failures >= this.failureLimit;
    const shouldNotify = paused && !current.notified;
    this.entry = {
      key,
      failures,
      paused,
      notified: current.notified || shouldNotify,
    };
    return paused ? { failures, shouldNotify } : null;
  }

  // fallow-ignore-next-line unused-class-member
  readonly recordSuccess = (key: string): void => {
    this.reset(key);
  };

  reset(key?: string): void {
    if (key === undefined || this.entry?.key === key) {
      this.entry = null;
    }
  }
}

export const buildAnalysisBackoffKey = (root: string, args: ReadonlyArray<string>): string =>
  JSON.stringify([root, ...args]);

/**
 * Variables that the CLI run of the sidebar reads and the language server
 * does not (`crates/engine/src/resolved_run/env_policy.rs`). In the editor the
 * `fallow.production` and `fallow.typeAware` settings decide, so the sidebar
 * run drops these variables of the extension host. Without that, a shell with
 * `FALLOW_PRODUCTION=1` gave the sidebar and the status bar different counts.
 * A Rust test in `env_policy.rs` checks this list.
 */
export const EDITOR_IGNORED_ENV_VARS = [
  "FALLOW_PRODUCTION",
  "FALLOW_PRODUCTION_DEAD_CODE",
  "FALLOW_PRODUCTION_HEALTH",
  "FALLOW_PRODUCTION_DUPES",
  "FALLOW_DIFF_FILE",
  "FALLOW_TYPE_AWARE",
  "FALLOW_TYPE_AWARE_PROJECTS",
  "FALLOW_TYPE_AWARE_REQUIRE",
] as const;

/**
 * The environment overrides of a Fallow child process that the editor starts.
 * `undefined` removes a variable: Node skips an `undefined` value of `env`.
 */
export type ChildEnvOverrides = Readonly<Record<string, string | undefined>>;

/**
 * The type-aware timeout override of a Fallow child process, for the CLI
 * runs and the language server alike. Unset leaves the default and any
 * OS-level variable untouched.
 */
export const typeAwareTimeoutEnv = (typeAwareTimeoutSeconds: number): ChildEnvOverrides =>
  Number.isInteger(typeAwareTimeoutSeconds) && typeAwareTimeoutSeconds > 0
    ? { FALLOW_TYPE_AWARE_TIMEOUT_SECS: String(typeAwareTimeoutSeconds) }
    : {};

export const buildAnalysisProcessEnv = (
  env: NodeJS.ProcessEnv = process.env,
  typeAwareTimeoutSeconds = 0,
): ChildEnvOverrides => {
  const configured = env.FALLOW_MAX_FILE_SIZE?.trim();
  const overrides: Record<string, string | undefined> = {
    FALLOW_MAX_FILE_SIZE:
      configured && configured.length > 0 ? configured : ANALYSIS_DEFAULT_MAX_FILE_SIZE_MB,
    ...typeAwareTimeoutEnv(typeAwareTimeoutSeconds),
  };
  // Windows keeps the casing of a name in a copy of `process.env`, so a
  // `Fallow_Production` key must go too.
  const ignored = new Set<string>(EDITOR_IGNORED_ENV_VARS);
  for (const name of Object.keys(env)) {
    if (ignored.has(name.toUpperCase())) {
      overrides[name] = undefined;
    }
  }
  return overrides;
};
