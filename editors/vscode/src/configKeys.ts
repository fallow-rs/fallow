export const RESTART_CONFIG_KEYS = [
  "fallow.lspPath",
  "fallow.configPath",
  "fallow.trace.server",
  "fallow.issueTypes",
  "fallow.changedSince",
  "fallow.duplication",
  "fallow.health.inlineComplexity",
  "fallow.autoDownload",
] as const;

export const REANALYSIS_CONFIG_KEYS = [
  "fallow.configPath",
  "fallow.production",
  "fallow.duplication",
  "fallow.issueTypes",
  "fallow.changedSince",
  // A pinned workspace-scope change re-runs the dead-code/dupes sidebar + status
  // bar so they reflect the new scope. Deliberately NOT in RESTART_CONFIG_KEYS:
  // the LSP is not workspace-scoped, so a workspace change must not restart it.
  "fallow.workspace",
] as const;

// Most health settings drive the separate lazy health spawn, not the LSP.
// `fallow.health.inlineComplexity` is the exception because it is forwarded to
// fallow-lsp as an initialization option and therefore lives in
// RESTART_CONFIG_KEYS.
export const HEALTH_CONFIG_KEYS = [
  "fallow.health.enabled",
  "fallow.health.hotspots",
  "fallow.health.topFindings",
  "fallow.health.statusBar",
  // The inline complexity breakdown is backed by the same health spawn:
  // enabling it (or changing the decoration cap) changes the spawn's args, so a
  // re-run is needed. `afterText` is render-only and handled separately.
  "fallow.complexity.breakdownEnabled",
  "fallow.complexity.decorationCap",
] as const;

export interface ConfigurationChangeLike {
  affectsConfiguration: (key: string) => boolean;
}

export const affectsAnyConfiguration = (
  event: ConfigurationChangeLike,
  keys: readonly string[],
): boolean => keys.some((key) => event.affectsConfiguration(key));
