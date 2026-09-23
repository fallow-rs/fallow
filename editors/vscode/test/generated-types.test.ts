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
 * in `crates/cli/src/report/json.rs` and by `pnpm run check:contracts`.
 */
import { describe, expect, expectTypeOf, it } from "vitest";
import type {
  AuditSchemaVersion,
  CheckOutput,
  CheckSchemaVersion,
  CombinedOutput,
  CombinedSchemaVersion,
  DupesSchemaVersion,
  DupesOutput,
  FeatureFlagsSchemaVersion,
  HealthSchemaVersion,
  HealthOutput,
  IssueAction,
  SchemaVersion,
  TypeAwareStatusSchemaVersion,
  UnusedFileFinding,
} from "../src/generated/output-contract.js";
import type { LspInitializationOptions } from "../src/generated/lsp-initialization-options.js";
import type { SecurityFinding, SecurityOutput } from "../src/types.js";

/**
 * `true` only for a narrow numeric type: a literal, or a union of literals.
 * A codegen regression that dropped a `const` / `enum` from the schema would
 * widen the generated alias to plain `number` and flip this to `false`.
 */
type IsNarrowNumber<T> = [T] extends [number] ? (number extends T ? false : true) : false;

/**
 * A value of a generated `*SchemaVersion` type without restating the number.
 * Nothing in these samples reads `schema_version`; only its presence and its
 * type are under test, so the samples stay valid across a legitimate bump.
 */
const schemaVersion = <T>(): T => 0 as unknown as T;

describe("generated/output-contract.d.ts", () => {
  /**
   * The version *numbers* are deliberately not asserted here. They are
   * generated artifacts: `pnpm run check:contracts` regenerates this file from
   * the JSON Schema and fails on drift, and the schema itself is asserted
   * against Rust by the schema-driven test in `crates/cli/src/report/json.rs`.
   * Restating the numbers would add no coverage over those two gates while
   * turning every legitimate envelope bump into a red `pnpm run lint`. What
   * this extension actually depends on is structural: every envelope keeps its
   * own narrow version type, so a widened alias cannot silently let the
   * extension accept an envelope it cannot read.
   */
  it("keeps each envelope independently and narrowly versioned", () => {
    expectTypeOf<SchemaVersion>().toEqualTypeOf<CheckSchemaVersion>();

    expectTypeOf<IsNarrowNumber<SchemaVersion>>().toEqualTypeOf<true>();
    expectTypeOf<IsNarrowNumber<AuditSchemaVersion>>().toEqualTypeOf<true>();
    expectTypeOf<IsNarrowNumber<CheckSchemaVersion>>().toEqualTypeOf<true>();
    expectTypeOf<IsNarrowNumber<CombinedSchemaVersion>>().toEqualTypeOf<true>();
    expectTypeOf<IsNarrowNumber<DupesSchemaVersion>>().toEqualTypeOf<true>();
    expectTypeOf<IsNarrowNumber<FeatureFlagsSchemaVersion>>().toEqualTypeOf<true>();
    expectTypeOf<IsNarrowNumber<HealthSchemaVersion>>().toEqualTypeOf<true>();
    expectTypeOf<IsNarrowNumber<TypeAwareStatusSchemaVersion>>().toEqualTypeOf<true>();

    // Sanity: the helper is not vacuously true.
    expectTypeOf<IsNarrowNumber<number>>().toEqualTypeOf<false>();
  });

  it("exposes the LSP initializationOptions contract sent by the extension", () => {
    const sample: LspInitializationOptions = {
      allowRemoteExtends: false,
      changedSince: "origin/main",
      configPath: "/workspace/.fallowrc.jsonc",
      duplication: {
        crossLanguage: true,
        ignoreImports: true,
        minLines: 8,
        minOccurrences: 3,
        minTokens: 80,
        mode: "semantic",
        near: true,
        skipLocal: true,
        threshold: 8,
      },
      issueTypes: {
        "unused-exports": false,
      },
      production: true,
    };

    expect(sample.issueTypes["unused-exports"]).toBe(false);
    expect(sample.mutedCategories).toBeUndefined();
  });

  it("exposes optional muted diagnostic categories for LSP clients", () => {
    expectTypeOf<LspInitializationOptions["mutedCategories"]>().toEqualTypeOf<
      readonly string[] | undefined
    >();

    const sample: Pick<LspInitializationOptions, "mutedCategories"> = {
      mutedCategories: ["code-duplication"],
    };
    expect(sample.mutedCategories).toEqual(["code-duplication"]);
  });

  it("exposes CombinedOutput with optional check/dupes/health branches", () => {
    const sample: CombinedOutput = {
      schema_version: schemaVersion<CombinedSchemaVersion>(),
      version: "0.0.0-test",
      elapsed_ms: 0,
    };
    expect(sample.check).toBeUndefined();
    expect(sample.dupes).toBeUndefined();
    expect(sample.health).toBeUndefined();
  });

  it("requires the schema_version / version / elapsed_ms / total_issues envelope on CheckOutput", () => {
    const sample: CheckOutput = {
      schema_version: schemaVersion<CheckSchemaVersion>(),
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
        deprecated_exports_in_use: 0,
        unused_dependencies: 0,
        unused_enum_members: 0,
        unused_class_members: 0,
        unresolved_imports: 0,
        unlisted_dependencies: 0,
        duplicate_exports: 0,
        type_only_dependencies: 0,
        test_only_dependencies: 0,
        dev_dependencies_in_production: 0,
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

  it("re-exports the SecurityOutput / SecurityFinding contract from ../src/types.js", () => {
    const finding: SecurityFinding = {
      finding_id: "security:src/app.tsx:12",
      kind: "client-server-leak",
      path: "src/app.tsx",
      line: 12,
      col: 0,
      evidence: "imports a server-only secret",
      severity: "high",
      trace: [{ path: "src/lib/secret.ts", line: 8, col: 0, role: "secret-source" }],
      actions: [],
      candidate: {
        sink: {
          path: "src/app.tsx",
          line: 12,
          col: 0,
        },
        boundary: {
          client_server: true,
          cross_module: false,
        },
      },
    };
    const sample: SecurityOutput = {
      schema_version: "3",
      version: "test",
      elapsed_ms: 0,
      config: {
        rules: {
          security_client_server_leak: { configured: "off", effective: "warn" },
          security_sink: { configured: "off", effective: "warn" },
        },
        categories_include: null,
        categories_exclude: null,
      },
      security_findings: [finding],
      unresolved_edge_files: 0,
      unresolved_callee_sites: 0,
    };
    expect(sample.security_findings[0]!.kind).toBe("client-server-leak");
  });
});
