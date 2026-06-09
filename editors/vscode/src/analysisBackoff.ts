export const ANALYSIS_FAILURE_LIMIT = 3;
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

  recordSuccess(key: string): void {
    this.reset(key);
  }

  reset(key?: string): void {
    if (key === undefined || this.entry?.key === key) {
      this.entry = null;
    }
  }
}

export const buildAnalysisBackoffKey = (
  root: string,
  args: ReadonlyArray<string>,
): string => JSON.stringify([root, ...args]);

export const buildAnalysisProcessEnv = (
  env: NodeJS.ProcessEnv = process.env,
): Readonly<Record<string, string>> => {
  const configured = env.FALLOW_MAX_FILE_SIZE?.trim();
  return {
    FALLOW_MAX_FILE_SIZE:
      configured && configured.length > 0 ? configured : ANALYSIS_DEFAULT_MAX_FILE_SIZE_MB,
  };
};
