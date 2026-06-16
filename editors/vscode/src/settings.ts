/**
 * VS Code settings types. These shape `settings.json` entries under the
 * `fallow.*` namespace and are kept in sync with `contributes.configuration`
 * in `package.json`. They are NOT part of fallow's JSON output contract and
 * therefore stay hand-written (not derived from `docs/output-schema.json`).
 */

export interface IssueTypeConfig {
  readonly "unused-files": boolean;
  readonly "unused-exports": boolean;
  readonly "unused-types": boolean;
  readonly "private-type-leaks": boolean;
  readonly "unused-dependencies": boolean;
  readonly "unused-dev-dependencies": boolean;
  readonly "unused-optional-dependencies": boolean;
  readonly "unused-enum-members": boolean;
  readonly "unused-class-members": boolean;
  readonly "unused-store-member": boolean;
  readonly "unused-server-action": boolean;
  readonly "unused-load-data-key": boolean;
  readonly "unused-component-prop": boolean;
  readonly "unused-component-emit": boolean;
  readonly "unused-component-input": boolean;
  readonly "unused-component-output": boolean;
  readonly "unrendered-component": boolean;
  readonly "unprovided-inject": boolean;
  readonly "invalid-client-export": boolean;
  readonly "mixed-client-server-barrel": boolean;
  readonly "misplaced-directive": boolean;
  readonly "route-collision": boolean;
  readonly "dynamic-segment-name-conflict": boolean;
  readonly "unresolved-imports": boolean;
  readonly "unlisted-dependencies": boolean;
  readonly "duplicate-exports": boolean;
  readonly "type-only-dependencies": boolean;
  readonly "test-only-dependencies": boolean;
  readonly "circular-dependencies": boolean;
  readonly "re-export-cycles": boolean;
  readonly "boundary-violation": boolean;
  readonly "policy-violation": boolean;
  readonly "stale-suppressions": boolean;
  readonly "unused-catalog-entries": boolean;
  readonly "unresolved-catalog-references": boolean;
  readonly "unused-dependency-overrides": boolean;
  readonly "misconfigured-dependency-overrides": boolean;
}

export type DuplicationMode = "strict" | "mild" | "weak" | "semantic";

export type DiagnosticSeveritySetting = "warning" | "information" | "hint";

export type TraceLevel = "off" | "messages" | "verbose";
