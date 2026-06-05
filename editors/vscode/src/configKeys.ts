export const RESTART_CONFIG_KEYS = [
  "fallow.lspPath",
  "fallow.configPath",
  "fallow.trace.server",
  "fallow.issueTypes",
  "fallow.changedSince",
  "fallow.duplication",
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

// Health is a separate, lazy spawn with its own latch, so its settings drive
// only a health re-run, never an LSP restart or a combined-analysis re-run.
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
