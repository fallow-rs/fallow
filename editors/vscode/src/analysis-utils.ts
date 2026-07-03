import type { DuplicationMode, FallowCheckResult, FallowDupesResult } from "./types.js";

/**
 * Analysis flags that did not exist in every CLI release the extension may
 * resolve, mapped to the first CLI version that accepts them. The extension and
 * the `fallow` binary are versioned and resolved independently (PATH,
 * node_modules/.bin, managed download, or a deliberately pinned binary), so a
 * newer extension can drive an older CLI. These flags are gated by version up
 * front and, as a backstop, are the ONLY flags `planDegradation` will strip
 * after a spawn failure. Anything else that a binary rejects stays loud.
 */
const VERSION_GATED_FLAGS: Readonly<Record<string, string>> = {
  // `--no-production` (force production OFF, the `fallow.production: "off"`
  // state) is new in 2.90.0; older CLIs only know `--production`. Gated so an
  // old pinned CLI degrades to deferring to the project config instead of
  // spawn-failing (issue #1055).
  "--no-production": "2.90.0",
  "--dupes-min-occurrences": "2.88.0",
  "--dupes-min-tokens": "2.88.3",
  "--dupes-min-lines": "2.88.3",
  "--dupes-skip-local": "2.88.3",
  "--dupes-cross-language": "2.88.3",
  "--dupes-ignore-imports": "2.88.3",
  // Opt-out of the default import exclusion (#1224). On older binaries
  // (< 2.96.0) the dupes default was already "count imports", so omitting the
  // flag yields the same behavior the user is asking for; degrade silently.
  "--dupes-no-ignore-imports": "2.96.0",
  "--complexity-breakdown": "2.89.0",
};

interface AnalysisArgsOptions {
  /**
   * Production-mode override: `true` forwards `--production`, `false` forwards
   * `--no-production` (force off, version-gated), `undefined` forwards neither
   * so the run defers to the project config. Mirrors the LSP init option so the
   * sidebar tree and editor squiggles agree (issue #1055).
   */
  readonly production: boolean | undefined;
  readonly changedSince: string;
  /**
   * Monorepo workspace scope (a package name). When non-empty, forwarded as
   * `--workspace <name>` so the combined run analyzes only that package. NOT
   * version-gated: `--workspace` is a long-standing global CLI flag.
   */
  readonly workspace: string;
  readonly configPath: string;
  readonly dupesMode: DuplicationMode | undefined;
  readonly dupesThreshold: number | undefined;
  readonly dupesMinTokens: number | undefined;
  readonly dupesMinLines: number | undefined;
  readonly minOccurrences: number | undefined;
  readonly dupesSkipLocal: boolean | undefined;
  readonly dupesCrossLanguage: boolean | undefined;
  readonly dupesIgnoreImports: boolean | undefined;
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

const pushVersionGatedFlag = (
  args: string[],
  skipped: SkippedFlag[],
  flag: string,
  cliVersion: string | null,
  value?: string,
): void => {
  const requires = VERSION_GATED_FLAGS[flag];
  if (cliVersion !== null && compareVersions(cliVersion, requires) < 0) {
    skipped.push({ flag, requires, cliVersion });
    return;
  }

  args.push(flag);
  if (value !== undefined) {
    args.push(value);
  }
};

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

  if (options.production === true) {
    args.push("--production");
  } else if (options.production === false) {
    pushVersionGatedFlag(args, skipped, "--no-production", options.cliVersion);
  }

  if (options.changedSince) {
    args.push("--changed-since", options.changedSince);
  }

  if (options.workspace) {
    args.push("--workspace", options.workspace);
  }

  if (options.configPath) {
    args.push("--config", options.configPath);
  }

  if (options.dupesMode !== undefined) {
    args.push("--dupes-mode", options.dupesMode);
  }

  if (options.dupesThreshold !== undefined) {
    args.push("--dupes-threshold", String(options.dupesThreshold));
  }

  if (options.dupesMinTokens !== undefined) {
    pushVersionGatedFlag(
      args,
      skipped,
      "--dupes-min-tokens",
      options.cliVersion,
      String(options.dupesMinTokens),
    );
  }

  if (options.dupesMinLines !== undefined) {
    pushVersionGatedFlag(
      args,
      skipped,
      "--dupes-min-lines",
      options.cliVersion,
      String(options.dupesMinLines),
    );
  }

  if (options.minOccurrences !== undefined) {
    pushVersionGatedFlag(
      args,
      skipped,
      "--dupes-min-occurrences",
      options.cliVersion,
      String(options.minOccurrences),
    );
  }

  if (options.dupesSkipLocal === true) {
    pushVersionGatedFlag(args, skipped, "--dupes-skip-local", options.cliVersion);
  }

  if (options.dupesCrossLanguage === true) {
    pushVersionGatedFlag(args, skipped, "--dupes-cross-language", options.cliVersion);
  }

  // `ignoreImports` defaults to true on the CLI (#1224); only forward a flag
  // when the user explicitly set the setting. `true` is redundant-but-valid;
  // `false` is the opt-out that the CLI default now requires a flag to express.
  if (options.dupesIgnoreImports === true) {
    pushVersionGatedFlag(args, skipped, "--dupes-ignore-imports", options.cliVersion);
  } else if (options.dupesIgnoreImports === false) {
    pushVersionGatedFlag(args, skipped, "--dupes-no-ignore-imports", options.cliVersion);
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
    (result.unused_store_members?.length ?? 0) +
    (result.unused_server_actions?.length ?? 0) +
    (result.unused_load_data_keys?.length ?? 0) +
    (result.unused_component_props?.length ?? 0) +
    (result.unused_component_emits?.length ?? 0) +
    (result.unused_component_inputs?.length ?? 0) +
    (result.unused_component_outputs?.length ?? 0) +
    (result.unused_svelte_events?.length ?? 0) +
    (result.unrendered_components?.length ?? 0) +
    (result.unprovided_injects?.length ?? 0) +
    (result.invalid_client_exports?.length ?? 0) +
    (result.mixed_client_server_barrels?.length ?? 0) +
    (result.misplaced_directives?.length ?? 0) +
    (result.route_collisions?.length ?? 0) +
    (result.dynamic_segment_name_conflicts?.length ?? 0) +
    result.unresolved_imports.length +
    result.unlisted_dependencies.length +
    result.duplicate_exports.length +
    (result.type_only_dependencies?.length ?? 0) +
    (result.test_only_dependencies?.length ?? 0) +
    (result.dev_dependencies_in_production?.length ?? 0) +
    (result.circular_dependencies?.length ?? 0) +
    (result.re_export_cycles?.length ?? 0) +
    (result.boundary_violations?.length ?? 0) +
    (result.boundary_coverage_violations?.length ?? 0) +
    (result.boundary_call_violations?.length ?? 0) +
    (result.policy_violations?.length ?? 0) +
    (result.stale_suppressions?.length ?? 0) +
    (result.unused_catalog_entries?.length ?? 0) +
    // empty_catalog_groups has no per-type toggle (it is not in IssueTypeConfig),
    // so it is always counted; omitting it under-reported total_issues and the
    // status-bar count.
    (result.empty_catalog_groups?.length ?? 0) +
    (result.unresolved_catalog_references?.length ?? 0) +
    (result.unused_dependency_overrides?.length ?? 0) +
    (result.misconfigured_dependency_overrides?.length ?? 0)
  );
};

export const countDiagnosticErrorIssues = (result: FallowCheckResult | null): number => {
  if (!result) {
    return 0;
  }

  // Only categories the LSP renders at DiagnosticSeverity::ERROR, so the badge's
  // "N errors" matches the red squiggles in the editor. The four RSC structural
  // checks (unprovided_injects, invalid_client_exports, mixed_client_server_barrels,
  // misplaced_directives) are emitted at WARNING (see
  // crates/lsp/src/diagnostics/structural.rs) and are deliberately excluded.
  return (
    result.unresolved_imports.length +
    (result.route_collisions?.length ?? 0) +
    (result.dynamic_segment_name_conflicts?.length ?? 0) +
    (result.policy_violations?.filter((finding) => finding.severity === "error").length ?? 0) +
    (result.unresolved_catalog_references?.length ?? 0) +
    (result.misconfigured_dependency_overrides?.length ?? 0)
  );
};

export const countDuplicationGroups = (result: FallowDupesResult | null): number => {
  if (!result) {
    return 0;
  }
  return result.stats.clone_groups;
};

export interface CleanAnalysisSummary {
  readonly notification: string;
  readonly outputLines: readonly string[];
}

const formatAnalyzedFiles = (count: number): string =>
  `${count} analyzed JS/TS file${count === 1 ? "" : "s"}`;

const formatDuplicationPercentage = (value: number): string => {
  if (!Number.isFinite(value)) {
    return "0%";
  }
  return `${value.toFixed(value === Math.trunc(value) ? 0 : 1)}%`;
};

export const buildCleanAnalysisSummary = (
  check: FallowCheckResult | null,
  dupes: FallowDupesResult | null,
): CleanAnalysisSummary => {
  if (!check) {
    return {
      notification: "Fallow: analysis completed, but no dead-code summary was available.",
      outputLines: [
        "Fallow analysis summary:",
        "- Dead code: summary unavailable.",
        "- Duplication: summary unavailable.",
      ],
    };
  }

  if (!dupes) {
    return {
      notification:
        "Fallow: no dead-code issues found in analyzed JS/TS files. Duplication summary unavailable.",
      outputLines: [
        "Fallow analysis summary:",
        "- Dead code: no issues found in analyzed JS/TS files.",
        "- Duplication: summary unavailable.",
      ],
    };
  }

  const files = formatAnalyzedFiles(dupes.stats.total_files);
  const pct = formatDuplicationPercentage(dupes.stats.duplication_percentage);
  return {
    notification: `Fallow: no issues found in analyzed JS/TS files (${files}).`,
    outputLines: [
      "Fallow analysis summary:",
      "- Dead code: no issues found in analyzed JS/TS files.",
      `- Duplication: no duplicate-code groups found across ${files} (${pct} duplicated lines).`,
    ],
  };
};
