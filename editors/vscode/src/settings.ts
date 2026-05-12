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
  readonly "unresolved-imports": boolean;
  readonly "unlisted-dependencies": boolean;
  readonly "duplicate-exports": boolean;
  readonly "type-only-dependencies": boolean;
  readonly "test-only-dependencies": boolean;
  readonly "circular-dependencies": boolean;
  readonly "boundary-violation": boolean;
  readonly "stale-suppressions": boolean;
  readonly "unused-catalog-entries": boolean;
}

export type DuplicationMode = "strict" | "mild" | "weak" | "semantic";

export type TraceLevel = "off" | "messages" | "verbose";
