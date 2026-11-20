export const RESTART_CONFIG_KEYS = [
  "fallow.lspPath",
  "fallow.configPath",
  "fallow.allowRemoteExtends",
  "fallow.trace.server",
  "fallow.issueTypes",
  "fallow.changedSince",
  "fallow.packageBaselines",
  "fallow.duplication",
  "fallow.typeAware",
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
  "fallow.allowRemoteExtends",
  "fallow.production",
  "fallow.duplication",
  "fallow.issueTypes",
  "fallow.changedSince",
  "fallow.packageBaselines",
  "fallow.typeAware",
  // A pinned workspace-scope change re-runs the dead-code/dupes sidebar + status
  // bar so they reflect the new scope. Deliberately NOT in RESTART_CONFIG_KEYS:
  // the LSP is not workspace-scoped, so a workspace change must not restart it.
  "fallow.workspace",
] as const;

// Health settings change how the extension renders health data: the context
// key, the status bar and the complexity decorations. None of them restart the
// LSP. This list does not decide when the health spawn starts again:
// `healthInputKey` in commands.ts decides that. `fallow.health.inlineComplexity`
// and `fallow.complexity.afterText` are render-only and handled in extension.ts.
export const HEALTH_CONFIG_KEYS = [
  "fallow.health.enabled",
  "fallow.health.hotspots",
  "fallow.health.topFindings",
  "fallow.health.statusBar",
  // The inline complexity breakdown renders from the same health report.
  "fallow.complexity.breakdownEnabled",
  "fallow.complexity.decorationCap",
  "fallow.typeAware",
] as const;

export const DIAGNOSTIC_RENDER_CONFIG_KEYS = ["fallow.diagnostics.severity"] as const;

export interface ConfigurationChangeLike {
  affectsConfiguration: (key: string) => boolean;
}

export const affectsAnyConfiguration = (
  event: ConfigurationChangeLike,
  keys: readonly string[],
): boolean => keys.some((key) => event.affectsConfiguration(key));
