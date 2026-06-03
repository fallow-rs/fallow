import { MIN_OCCURRENCES_FLOOR } from "./duplication-utils.js";
import type { DuplicationMode, FallowCheckResult } from "./types.js";

/**
 * Analysis flags that did not exist in every CLI release the extension may
 * resolve, mapped to the first CLI version that accepts them. The extension and
 * the `fallow` binary are versioned and resolved independently (PATH,
 * node_modules/.bin, managed download, or a deliberately pinned binary), so a
 * newer extension can drive an older CLI. These flags are gated by version up
 * front and, as a backstop, are the ONLY flags `planDegradation` will strip
 * after a spawn failure. Anything else that a binary rejects stays loud.
 */
export const VERSION_GATED_FLAGS: Readonly<Record<string, string>> = {
  "--dupes-min-occurrences": "2.88.0",
};

interface AnalysisArgsOptions {
  readonly production: boolean;
  readonly changedSince: string;
  readonly configPath: string;
  readonly dupesMode: DuplicationMode;
  readonly dupesThreshold: number;
  readonly minOccurrences: number;
  /**
   * Version of the resolved CLI (`getBinaryVersion`), or null when it could not
   * be probed. When known, version-gated flags below their introducing version
   * are omitted up front rather than spawn-failed.
   */
  readonly cliVersion: string | null;
}

/** A flag omitted up front because the resolved CLI is too old to accept it. */
export interface SkippedFlag {
  readonly flag: string;
  readonly requires: string;
  readonly cliVersion: string;
}

export interface BuiltAnalysisArgs {
  readonly args: string[];
  readonly skipped: readonly SkippedFlag[];
}

/**
 * Compare two dotted numeric versions. Returns a negative number when `a < b`,
 * zero when equal, positive when `a > b`. Missing or non-numeric segments are
 * treated as 0; any pre-release suffix is ignored (we only gate on the X.Y.Z
 * core, matching what `getBinaryVersion` parses out of `--version`).
 */
export const compareVersions = (a: string, b: string): number => {
  const parse = (v: string): number[] =>
    v.split(".").map((segment) => Number.parseInt(segment, 10) || 0);
  const pa = parse(a);
  const pb = parse(b);
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i += 1) {
    const diff = (pa[i] ?? 0) - (pb[i] ?? 0);
    if (diff !== 0) {
      return diff;
    }
  }
  return 0;
};

/**
 * Build the argument vector for the combined `fallow` analysis run that backs
 * the sidebar. Kept pure (no config/VS Code access) so flag-forwarding rules
 * can be unit-tested. Returns the argv plus any version-gated flags that were
 * omitted because the resolved CLI is too old, so the caller can tell the user
 * their setting was not applied.
 */
export const buildAnalysisArgs = (options: AnalysisArgsOptions): BuiltAnalysisArgs => {
  const args = ["--format", "json", "--quiet", "--skip", "health"];
  const skipped: SkippedFlag[] = [];

  if (options.production) {
    args.push("--production");
  }

  if (options.changedSince) {
    args.push("--changed-since", options.changedSince);
  }

  if (options.configPath) {
    args.push("--config", options.configPath);
  }

  args.push("--dupes-mode", options.dupesMode);
  args.push("--dupes-threshold", String(options.dupesThreshold));

  // `--dupes-min-occurrences` (CLI v2.88.0+). The floor (2) is also the CLI
  // default, so a default value is a no-op we simply omit. When the user raised
  // it, forward the flag only if the resolved CLI is new enough; if we know the
  // CLI predates it, record the skip so the caller can warn that the setting was
  // not honored. When the version is unknown, forward optimistically and let the
  // spawn-failure backstop handle a genuinely too-old binary.
  if (options.minOccurrences > MIN_OCCURRENCES_FLOOR) {
    const flag = "--dupes-min-occurrences";
    const requires = VERSION_GATED_FLAGS[flag];
    if (options.cliVersion !== null && compareVersions(options.cliVersion, requires) < 0) {
      skipped.push({ flag, requires, cliVersion: options.cliVersion });
    } else {
      args.push(flag, String(options.minOccurrences));
    }
  }

  return { args, skipped };
};

/**
 * Extract the offending flag from a clap "unexpected argument" failure so the
 * caller can strip it and retry against an older binary. Handles both modern
 * clap (`unexpected argument '--x' found`) and legacy clap 3.x / early-4.x
 * (`Found argument '--x' which wasn't expected`), since a pinned binary is old
 * by definition. Returns null when the error is unrelated to an unknown flag
 * (real failures must still surface).
 */
export const parseUnexpectedArgument = (message: string): string | null => {
  const modern = /unexpected argument '(-{1,2}[^']+)'/.exec(message);
  if (modern) {
    return modern[1];
  }
  const legacy = /Found argument '(-{1,2}[^']+)' which wasn't expected/.exec(message);
  if (legacy) {
    return legacy[1];
  }
  return null;
};

/**
 * Remove `flag` (and its space-separated value, if any) from an argument
 * vector. Handles both `--flag value` and `--flag=value` spellings.
 */
export const stripArgument = (args: ReadonlyArray<string>, flag: string): string[] => {
  const result: string[] = [];
  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i];
    if (arg === flag) {
      // Our analysis flags are `--flag value`; drop the trailing value when the
      // next token is not itself a flag.
      const next = args[i + 1];
      if (next !== undefined && !next.startsWith("-")) {
        i += 1;
      }
      continue;
    }
    if (arg.startsWith(`${flag}=`)) {
      continue;
    }
    result.push(arg);
  }
  return result;
};

export type DegradationPlan =
  | { readonly kind: "rethrow" }
  | { readonly kind: "retry"; readonly args: string[]; readonly dropped: string };

/**
 * Decide, purely, how to react to a failed analysis spawn. If the error is a
 * clap "unexpected argument" naming one of our known VERSION_GATED_FLAGS, return
 * a retry with that flag stripped; otherwise rethrow so genuine failures (real
 * bugs, a typo'd flag, a corrupt binary) stay loud. The allowlist is what keeps
 * graceful degradation from masking unrelated errors.
 */
export const planDegradation = (
  errorMessage: string,
  args: ReadonlyArray<string>,
): DegradationPlan => {
  const offending = parseUnexpectedArgument(errorMessage);
  if (!offending || !Object.hasOwn(VERSION_GATED_FLAGS, offending)) {
    return { kind: "rethrow" };
  }
  const reduced = stripArgument(args, offending);
  if (reduced.length === args.length) {
    // Nothing was stripped (the flag is not actually in our argv); surface the
    // error rather than spin.
    return { kind: "rethrow" };
  }
  return { kind: "retry", args: reduced, dropped: offending };
};

export const countCheckIssues = (result: FallowCheckResult | null): number => {
  if (!result) {
    return 0;
  }

  return (
    result.unused_files.length +
    result.unused_exports.length +
    result.unused_types.length +
    (result.private_type_leaks?.length ?? 0) +
    result.unused_dependencies.length +
    result.unused_dev_dependencies.length +
    (result.unused_optional_dependencies?.length ?? 0) +
    result.unused_enum_members.length +
    result.unused_class_members.length +
    result.unresolved_imports.length +
    result.unlisted_dependencies.length +
    result.duplicate_exports.length +
    (result.type_only_dependencies?.length ?? 0) +
    (result.test_only_dependencies?.length ?? 0) +
    (result.circular_dependencies?.length ?? 0) +
    (result.re_export_cycles?.length ?? 0) +
    (result.boundary_violations?.length ?? 0) +
    (result.stale_suppressions?.length ?? 0) +
    (result.unused_catalog_entries?.length ?? 0) +
    (result.unresolved_catalog_references?.length ?? 0) +
    (result.unused_dependency_overrides?.length ?? 0) +
    (result.misconfigured_dependency_overrides?.length ?? 0)
  );
};
