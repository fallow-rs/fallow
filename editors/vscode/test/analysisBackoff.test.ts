import { describe, expect, it } from "vitest";
import {
  ANALYSIS_DEFAULT_MAX_FILE_SIZE_MB,
  AnalysisFailureBackoff,
  buildAnalysisBackoffKey,
  buildAnalysisProcessEnv,
  EDITOR_IGNORED_ENV_VARS,
} from "../src/analysisBackoff.js";

describe("AnalysisFailureBackoff", () => {
  it("pauses automatic retries after repeated failures for the same workspace input", () => {
    const backoff = new AnalysisFailureBackoff();
    const key = buildAnalysisBackoffKey("/repo", ["--format", "json"]);

    expect(backoff.recordFailure(key)).toBeNull();
    expect(backoff.recordFailure(key)).toBeNull();
    expect(backoff.recordFailure(key)).toEqual({ failures: 3, shouldNotify: true });
    expect(backoff.blockedNotice(key, false)).toEqual({ failures: 3, shouldNotify: false });
  });

  it("lets manual retries clear a paused input", () => {
    const backoff = new AnalysisFailureBackoff();
    const key = buildAnalysisBackoffKey("/repo", ["--format", "json"]);

    backoff.recordFailure(key);
    backoff.recordFailure(key);
    backoff.recordFailure(key);

    expect(backoff.blockedNotice(key, true)).toBeNull();
    expect(backoff.recordFailure(key)).toBeNull();
  });

  it("tracks changed analysis input independently", () => {
    const backoff = new AnalysisFailureBackoff();
    const first = buildAnalysisBackoffKey("/repo", ["--format", "json"]);
    const second = buildAnalysisBackoffKey("/repo", [
      "--format",
      "json",
      "--changed-since",
      "main",
    ]);

    backoff.recordFailure(first);
    backoff.recordFailure(first);
    backoff.recordFailure(first);

    expect(backoff.blockedNotice(second, false)).toBeNull();
  });
});

describe("buildAnalysisProcessEnv", () => {
  it("adds the default max-file-size ceiling when the user has not configured one", () => {
    expect(buildAnalysisProcessEnv({})).toEqual({
      FALLOW_MAX_FILE_SIZE: ANALYSIS_DEFAULT_MAX_FILE_SIZE_MB,
    });
  });

  it("preserves an explicit user max-file-size ceiling", () => {
    expect(buildAnalysisProcessEnv({ FALLOW_MAX_FILE_SIZE: "2" })).toEqual({
      FALLOW_MAX_FILE_SIZE: "2",
    });
  });

  it("forwards a configured semantic timeout", () => {
    expect(buildAnalysisProcessEnv({}, 600)).toEqual({
      FALLOW_MAX_FILE_SIZE: ANALYSIS_DEFAULT_MAX_FILE_SIZE_MB,
      FALLOW_TYPE_AWARE_TIMEOUT_SECS: "600",
    });
  });

  it("leaves the semantic timeout to the CLI and the environment when unset", () => {
    for (const unset of [0, -30, 1.5, Number.NaN]) {
      expect(buildAnalysisProcessEnv({ FALLOW_TYPE_AWARE_TIMEOUT_SECS: "900" }, unset)).toEqual({
        FALLOW_MAX_FILE_SIZE: ANALYSIS_DEFAULT_MAX_FILE_SIZE_MB,
      });
    }
  });

  it("drops the variables that the language server does not read", () => {
    // In the editor the settings decide. A shell FALLOW_PRODUCTION=1 gave the
    // sidebar run production mode and the status bar not.
    const env = {
      FALLOW_PRODUCTION: "1",
      FALLOW_TYPE_AWARE: "1",
      FALLOW_DIFF_FILE: "/tmp/pr.diff",
      FALLOW_CACHE_DIR: "/tmp/cache",
    };
    const overrides = buildAnalysisProcessEnv(env);
    expect(overrides).toEqual({
      FALLOW_MAX_FILE_SIZE: ANALYSIS_DEFAULT_MAX_FILE_SIZE_MB,
      FALLOW_PRODUCTION: undefined,
      FALLOW_TYPE_AWARE: undefined,
      FALLOW_DIFF_FILE: undefined,
    });
    expect("FALLOW_PRODUCTION" in overrides).toBe(true);
    for (const name of EDITOR_IGNORED_ENV_VARS) {
      expect(buildAnalysisProcessEnv({ [name]: "x" })[name]).toBeUndefined();
    }
    const mixedCase = buildAnalysisProcessEnv({ Fallow_Production: "1" });
    expect("Fallow_Production" in mixedCase).toBe(true);
    expect(mixedCase.Fallow_Production).toBeUndefined();
  });
});
