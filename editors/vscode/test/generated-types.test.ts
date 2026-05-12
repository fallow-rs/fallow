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
  UnusedFile,
} from "../src/generated/output-contract.js";

describe("generated/output-contract.d.ts", () => {
  it("exposes CombinedOutput with optional check/dupes/health branches", () => {
    const sample: CombinedOutput = {
      schema_version: 6,
      version: "0.0.0-test",
      elapsed_ms: 0,
    };
    expect(sample.check).toBeUndefined();
    expect(sample.dupes).toBeUndefined();
    expect(sample.health).toBeUndefined();
  });

  it("requires the schema_version / version / elapsed_ms / total_issues envelope on CheckOutput", () => {
    const sample: CheckOutput = {
      schema_version: 6,
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
    };
    expect(sample.total_issues).toBe(0);
  });

  it("describes DupesOutput and HealthOutput as object shapes", () => {
    const dupes: Partial<DupesOutput> = {};
    const health: Partial<HealthOutput> = {};
    expect(dupes).toEqual({});
    expect(health).toEqual({});
  });

  it("ties UnusedFile.actions[] to the IssueAction discriminated union", () => {
    const sample: UnusedFile = {
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
