/**
 * Regression sentinel for `src/generated/output-contract.d.ts`. This test
 * does NOT try to mirror the full schema; that would just duplicate the
 * contract. Instead it asserts that a handful of structural invariants
 * survive a regeneration, so accidental changes to codegen config
 * (`additionalProperties` flipping, `customName` regression, banner change,
 * a forgotten preprocessor pass) fail loudly in `pnpm run test:unit`
 * before they get committed.
 *
 * Drift between Rust and the schema is enforced by the schema-driven test
 * in `crates/cli/src/report/json.rs` and by `pnpm run check:codegen`.
 */
import { describe, expect, it } from "vitest";
import type {
  CheckOutput,
  CombinedOutput,
  DupesOutput,
  HealthOutput,
  IssueAction,
  UnusedFileFinding,
} from "../src/generated/output-contract.js";

describe("generated/output-contract.d.ts", () => {
  it("exposes CombinedOutput with optional check/dupes/health branches", () => {
    const sample: CombinedOutput = {
      kind: "combined",
      schema_version: 7,
      version: "0.0.0-test",
      elapsed_ms: 0,
    };
    expect(sample.check).toBeUndefined();
    expect(sample.dupes).toBeUndefined();
    expect(sample.health).toBeUndefined();
  });

  it("requires the schema_version / version / elapsed_ms / total_issues envelope on CheckOutput", () => {
    const sample: CheckOutput = {
      kind: "dead-code",
      schema_version: 7,
      version: "0.0.0-test",
      elapsed_ms: 0,
      total_issues: 0,
      unused_files: [],
      unused_exports: [],
      unused_types: [],
      private_type_leaks: [],
      unused_dependencies: [],
      unused_dev_dependencies: [],
      unused_optional_dependencies: [],
      unused_enum_members: [],
      unused_class_members: [],
      unresolved_imports: [],
      unlisted_dependencies: [],
      duplicate_exports: [],
      type_only_dependencies: [],
      test_only_dependencies: [],
      circular_dependencies: [],
      boundary_violations: [],
      stale_suppressions: [],
      summary: {
        total_issues: 0,
        unused_files: 0,
        unused_exports: 0,
        unused_types: 0,
        private_type_leaks: 0,
        unused_dependencies: 0,
        unused_enum_members: 0,
        unused_class_members: 0,
        unresolved_imports: 0,
        unlisted_dependencies: 0,
        duplicate_exports: 0,
        type_only_dependencies: 0,
        test_only_dependencies: 0,
        circular_dependencies: 0,
        boundary_violations: 0,
        stale_suppressions: 0,
        unused_catalog_entries: 0,
        empty_catalog_groups: 0,
        unresolved_catalog_references: 0,
        unused_dependency_overrides: 0,
        misconfigured_dependency_overrides: 0,
      },
    };
    expect(sample.total_issues).toBe(0);
  });

  it("describes DupesOutput and HealthOutput as object shapes", () => {
    const dupes: Partial<DupesOutput> = {};
    const health: Partial<HealthOutput> = {};
    expect(dupes).toEqual({});
    expect(health).toEqual({});
  });

  it("ties UnusedFileFinding.actions[] to the IssueAction discriminated union", () => {
    const sample: UnusedFileFinding = {
      path: "src/foo.ts",
      actions: [
        {
          type: "delete-file",
          auto_fixable: true,
          description: "Delete this unused file",
        },
        {
          type: "suppress-line",
          auto_fixable: false,
          description: "Add an inline suppression comment",
          comment: "// fallow-ignore-next-line unused-file",
        },
      ],
    };
    expect(sample.actions).toHaveLength(2);
    const first: IssueAction = sample.actions[0]!;
    expect(first.type).toBe("delete-file");
  });
});
