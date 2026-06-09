import { describe, expect, it } from "vitest";
import {
  ANALYSIS_DEFAULT_MAX_FILE_SIZE_MB,
  AnalysisFailureBackoff,
  buildAnalysisBackoffKey,
  buildAnalysisProcessEnv,
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
});
