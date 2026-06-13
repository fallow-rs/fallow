export const RESTART_CONFIG_KEYS = [
  "fallow.lspPath",
  "fallow.configPath",
  "fallow.trace.server",
  "fallow.issueTypes",
  "fallow.changedSince",
  "fallow.duplication",
  // `fallow.production` is forwarded to the LSP via initializationOptions, which
  // the server only reads at startup, so a change must restart it (issue #1055).
  "fallow.production",
  // `fallow.autoDownload` restarts so that enabling it can resolve + download a
  // managed binary when none was found, and disabling it can fall back to a
  // PATH/local binary. It re-runs `resolveBinaryPath`, which may pick a different
  // binary than the running one. When an already-installed managed binary is the
  // resolved choice either way, the restart re-resolves to the byte-identical
  // path (a harmless no-op clearing of the Problems panel); a path-diffing guard
  // would avoid that but adds complexity for a rare, low-cost case.
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

// Health settings drive the separate lazy health spawn, not the LSP, so none
// of them restart it. `fallow.health.inlineComplexity` toggles the extension's
// own complexity lens and is handled as a render-only refresh in extension.ts
// (not here), so it is in neither RESTART_CONFIG_KEYS nor this list.
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

export const DIAGNOSTIC_RENDER_CONFIG_KEYS = ["fallow.diagnostics.severity"] as const;

export interface ConfigurationChangeLike {
  affectsConfiguration: (key: string) => boolean;
}

export const affectsAnyConfiguration = (
  event: ConfigurationChangeLike,
  keys: readonly string[],
): boolean => keys.some((key) => event.affectsConfiguration(key));
