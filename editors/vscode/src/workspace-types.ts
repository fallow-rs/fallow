/**
 * Types for `fallow workspaces --format json` output. This command's shape is
 * not covered by `docs/output-schema.json`, so these stay hand-written
 * (mirroring `fix-types.ts`). The `workspaces` subcommand emits the package
 * list the monorepo workspace picker scopes analysis to via the global
 * `--workspace <name>` flag.
 */

/** One workspace package as emitted by `fallow workspaces --format json`. */
export interface WorkspaceInfo {
  /** The package name from its `package.json` (the value `--workspace` matches). */
  readonly name: string;
  /** Workspace-relative path to the package directory. */
  readonly path: string;
  /**
   * `true` when the package is a generated / platform-specific artefact
   * (e.g. an npm optionalDependency platform package). The picker demotes
   * these so the real, hand-authored packages surface first.
   */
  readonly is_internal_dependency: boolean;
}

/** Envelope for `fallow workspaces --format json`. */
export interface WorkspacesOutput {
  readonly workspace_count: number;
  readonly workspaces: ReadonlyArray<WorkspaceInfo>;
}
