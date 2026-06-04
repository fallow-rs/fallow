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
] as const;

// Health is a separate, lazy spawn with its own latch, so its settings drive
// only a health re-run, never an LSP restart or a combined-analysis re-run.
export const HEALTH_CONFIG_KEYS = [
  "fallow.health.enabled",
  "fallow.health.hotspots",
  "fallow.health.topFindings",
  "fallow.health.statusBar",
] as const;

export interface ConfigurationChangeLike {
  affectsConfiguration: (key: string) => boolean;
}

export const affectsAnyConfiguration = (
  event: ConfigurationChangeLike,
  keys: readonly string[],
): boolean => keys.some((key) => event.affectsConfiguration(key));
