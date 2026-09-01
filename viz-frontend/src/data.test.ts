import { describe, expect, it } from "vitest";
import { runSearch } from "./state";
import type { AppState } from "./state";
import {
  basename,
  analysisAvailability,
  buildIndex,
  dirname,
  reachSet,
  reachSetMulti,
  dupRatio,
  formatSize,
  findingsForFile,
  legendText,
  lensColor,
  lensFindingLevel,
  healthRiskForFile,
  securityCandidatesForFile,
  securityRuntimeAvailability,
} from "./data";
import { getTheme } from "./theme";
import type { Lens, VizData, VizFile } from "./types";

const file = (over: Partial<VizFile> = {}): VizFile => ({
  path: "src/a.ts",
  size: 340,
  status: "clean",
  export_count: 1,
  unused_export_count: 0,
  is_entry: false,
  importer_count: 1,
  import_count: 0,
  fn_count: 1,
  max_cyclomatic: 1,
  max_cognitive: 1,
  react_hooks: 0,
  jsx_depth: 0,
  dup_lines: 0,
  in_cycle: false,
  ...over,
});

const data = (over: Partial<VizData> = {}): VizData => ({
  root: "demo",
  files: [file({ path: "src/a.ts" }), file({ path: "src/b.ts" }), file({ path: "lib/c.ts" })],
  edges: [
    [0, 1, 0],
    [1, 2, 0],
  ],
  summary: {
    total_files: 3,
    total_size: 1020,
    total_edges: 2,
    unused_files: 0,
    unused_exports: 0,
    unused_types: 0,
    unused_deps: 0,
    unresolved_imports: 0,
    circular_deps: 0,
    clone_groups: 0,
    duplicated_lines: 0,
    boundary_violations: 0,
    hotspot_files: 0,
  },
  workspaces: [],
  zones: [],
  cycles: [],
  clones: [],
  violations: [],
  architecture: {
    availability: { state: "complete", count: 0, unit: "violations" },
    findings: [],
  },
  dependencies: {
    availability: { state: "complete", count: 0, unit: "findings" },
    findings: [],
  },
  health: {
    availability: { state: "complete", count: 0, unit: "files" },
    capabilities: {
      complexity: { state: "complete", count: 0, unit: "findings" },
      maintainability: { state: "complete", count: 0, unit: "files" },
      crap: { state: "complete", count: 0, unit: "files" },
      coverage: { state: "complete", count: 0, unit: "gaps" },
      churn: { state: "unavailable", count: 0, unit: "files" },
      hotspots: { state: "unavailable", count: 0, unit: "files" },
      ownership: { state: "unavailable", count: 0, unit: "files" },
    },
    files: [],
    findings: [],
  },
  security: {
    availability: { state: "complete", count: 0, unit: "candidates" },
    runtime_availability: { state: "unavailable", count: 0, unit: "observations" },
    candidates: [],
    blind_spot_count: 0,
    blind_spots: [],
  },
  frameworks: {
    availability: { state: "complete", count: 0, unit: "findings" },
    detector_availability: { state: "complete", count: 0, unit: "detectors" },
    findings: [],
    detected_frameworks: [],
    detectors: [],
  },
  styling: {
    availability: { state: "complete", count: 0, unit: "findings" },
    findings: [],
  },
  feature_flags: {
    availability: { state: "complete", count: 0, unit: "flags" },
    findings: [],
  },
  ...over,
});

describe("buildIndex", () => {
  it("mirrors edges into importer and import lists", () => {
    const index = buildIndex(data());
    expect(index.importsOf[0]).toEqual([1]);
    expect(index.importersOf[1]).toEqual([0]);
    expect(index.importersOf[0]).toEqual([]);
  });

  it("marks every directed pair of a cycle in both directions", () => {
    const index = buildIndex(data({ cycles: [[0, 1]] }));
    const n = 3;
    expect(index.cycleEdges.has(0 * n + 1)).toBe(true);
    expect(index.cycleEdges.has(1 * n + 0)).toBe(true);
  });

  it("collects violation sources", () => {
    const index = buildIndex(
      data({
        violations: [{ from: 0, to: 2, from_zone: 0, to_zone: 1, line: 3, specifier: "../lib/c" }],
      }),
    );
    expect(index.violationSources.has(0)).toBe(true);
    expect(index.violationSources.has(1)).toBe(false);
  });

  it("ignores cycle pairs and violations that point outside the file table", () => {
    const index = buildIndex(
      data({
        cycles: [[0, 99]],
        violations: [{ from: 0, to: 99, from_zone: 0, to_zone: 1, line: 3, specifier: "x" }],
      }),
    );
    expect(index.cycleEdges.size).toBe(0);
    expect(index.violationSources.has(0)).toBe(false);
    expect(index.violationEdges.size).toBe(0);
  });

  it("builds a directory tree that survives chain collapsing", () => {
    const index = buildIndex(data());
    expect(index.nodesByPath.has("src")).toBe(true);
    expect(index.tree.size).toBeGreaterThan(0);
  });
});

describe("analysis contract adapters", () => {
  it("keeps complete zero findings distinct from unavailable analysis", () => {
    const d = {
      ...data(),
      security: {
        availability: { state: "complete", count: 0, unit: "candidates" },
        runtime_availability: {
          state: "unavailable",
          count: 0,
          unit: "observations",
          reason: "No runtime profile was supplied",
        },
        candidates: [],
        blind_spot_count: 0,
        blind_spots: [],
      },
      frameworks: {
        availability: {
          state: "disabled",
          count: 0,
          unit: "findings",
          reason: "Framework analysis is disabled",
        },
        findings: [],
      },
    } as unknown as VizData;
    expect(analysisAvailability(d, "security")).toMatchObject({
      state: "complete",
      count: 0,
      unit: "candidates",
    });
    expect(analysisAvailability(d, "frameworks")).toMatchObject({
      state: "disabled",
      reason: "Framework analysis is disabled",
    });
    expect(securityRuntimeAvailability(d)).toMatchObject({
      state: "unavailable",
      reason: "No runtime profile was supplied",
    });
    d.security.availability = {
      state: "unavailable",
      count: 0,
      unit: "candidates",
      reason: "Security artifacts were not retained",
    };
    expect(legendText("security", d, "graph")).toContain("Security artifacts were not retained");
  });

  it("normalizes file Security evidence without calling candidates vulnerabilities", () => {
    const d = {
      ...data(),
      security: {
        availability: { state: "complete", count: 1, unit: "candidates" },
        runtime_availability: { state: "disabled", count: 0, unit: "observations" },
        blind_spots: [],
        candidates: [
          {
            id: "sql-1",
            kind: "sql-injection",
            category: "Injection",
            cwe: 89,
            file: 0,
            path: "src/a.ts",
            line: 12,
            col: 4,
            evidence: "request input reaches a query builder",
            severity: "high",
            taint_confidence: "strong",
            source_kind: "request parameter",
            sink: "db.query",
            url_shape: "/users/:id",
            network_destination: "database",
            reachable_from_entry: true,
            reachable_from_untrusted_source: true,
            blast_radius: 7,
            crosses_boundary: true,
            client_server_boundary: false,
            cross_module_boundary: true,
            architecture_zone: "data",
            dead_code: false,
            runtime: { observed: true },
            taint_flow: {
              source: { path: "src/a.ts", line: 4, col: 2 },
              sink: { path: "src/a.ts", line: 12, col: 4 },
              intra_module: true,
              cross_module_hops: 0,
            },
            observed_controls: ["schema validation"],
            control_verification_prompt: "Confirm the schema rejects SQL metacharacters",
            trace: [
              { path: "src/a.ts", line: 4, col: 2, role: "source" },
              { path: "src/a.ts", line: 12, col: 4, role: "sink" },
            ],
            actions: [{ label: "Verify", command: "fallow security --trace sql-1" }],
          },
        ],
        blind_spot_count: 0,
      },
    } as unknown as VizData;
    const candidate = securityCandidatesForFile(d, 0)[0];
    expect(candidate).toMatchObject({
      id: "sql-1",
      title: "Injection",
      cwe: "CWE-89",
      confidence: "strong",
      source: "request parameter",
      sink: "db.query",
      urlShape: "/users/:id",
      networkDestination: "database",
      reachability: "entry point, untrusted source",
      blastRadius: 7,
      deadCode: false,
      observedControls: ["schema validation"],
      verificationPrompt: "Confirm the schema rejects SQL metacharacters",
    });
    expect(candidate.boundary).toContain("module boundary");
    expect(candidate.trace).toEqual(["source: src/a.ts:4:2", "sink: src/a.ts:12:4"]);
    expect(candidate.actions).toContainEqual({
      label: "Verify",
      command: "fallow security --trace sql-1",
    });
    expect(candidate.actions).toContainEqual({
      label: "Verify",
      command: 'fallow security --file "src/a.ts"',
    });
    const index = buildIndex(d);
    expect(lensColor("security", getTheme(true), index, d.files[0])).toBe(getTheme(true).red);
  });

  it("uses retained Health file metrics for coloring and finding levels", () => {
    const d = {
      ...data(),
      health: {
        availability: { state: "complete", count: 1, unit: "files" },
        files: [
          {
            file: 0,
            path: "src/a.ts",
            maintainability_index: 51,
            crap_max: 28,
            complexity_density: 0.4,
            fan_in: 2,
            fan_out: 3,
          },
        ],
        findings: [],
      },
    } as unknown as VizData;
    expect(healthRiskForFile(d, 0)).toBe(28);
    const index = buildIndex(d);
    expect(lensFindingLevel("health" as Lens, index, d.files[0], 0)).toBe(2);
    expect(lensFindingLevel("health" as Lens, index, d.files[1], 1)).toBe(0);
    expect(lensColor("health", getTheme(true), index, d.files[0])).not.toBe(
      getTheme(true).cellNeutral,
    );
  });

  it("associates one finding with every referenced file", () => {
    const d = data({
      architecture: {
        availability: { state: "complete", count: 1, unit: "violations" },
        findings: [
          {
            kind: "cross-package-cycle",
            title: "Cross-package cycle",
            file: 0,
            files: [0, 1],
            path: "src/a.ts",
            paths: ["src/a.ts", "src/b.ts"],
            description: "Two packages form a cycle",
            actions: [],
          },
        ],
      },
    });
    expect(findingsForFile(d, "architecture", 0)).toHaveLength(1);
    expect(findingsForFile(d, "architecture", 1)).toHaveLength(1);
  });
});

describe("dupRatio", () => {
  it("is zero without duplicated lines and capped at one", () => {
    expect(dupRatio(file())).toBe(0);
    expect(dupRatio(file({ size: 34, dup_lines: 500 }))).toBe(1);
  });
});

describe("legendText", () => {
  it("explains the neutral map when a finding lens is clean", () => {
    expect(legendText("unused", data(), "graph")).toContain("No findings");
  });

  it("keeps the color key when findings exist", () => {
    const d = data();
    d.summary.unused_files = 2;
    d.files[0].status = "unused";
    expect(legendText("unused", d, "graph")).toContain("Red is never imported");
  });

  it("describes tiles in map view and dots in graph view", () => {
    expect(legendText("overview", data(), "map")).toContain("Each tile is a file");
    expect(legendText("overview", data(), "graph")).toContain("Each dot is a file");
  });
});

describe("lens coloring", () => {
  const theme = getTheme(true);

  it("keeps the overview neutral except entry points", () => {
    const index = buildIndex(data());
    expect(lensColor("overview", theme, index, file())).toBe(theme.cellNeutral);
    expect(lensColor("overview", theme, index, file({ status: "entryPoint" }))).toBe(
      theme.cellEntry,
    );
  });

  it("grades findings per lens for the non-color texture channel", () => {
    const index = buildIndex(data());
    // unused: unused file severe, unused exports mild, clean none.
    expect(lensFindingLevel("unused", index, file({ status: "unused" }), 0)).toBe(2);
    expect(lensFindingLevel("unused", index, file({ unused_export_count: 2 }), 0)).toBe(1);
    expect(lensFindingLevel("unused", index, file(), 0)).toBe(0);
    // duplication: >= 30% duplicated lines severe, any duplication mild.
    expect(lensFindingLevel("duplication", index, file({ size: 340, dup_lines: 9 }), 0)).toBe(2);
    expect(lensFindingLevel("duplication", index, file({ size: 3400, dup_lines: 1 }), 0)).toBe(1);
    expect(lensFindingLevel("duplication", index, file(), 0)).toBe(0);
    // Health without retained file metrics stays neutral.
    expect(lensFindingLevel("health", index, file({ max_cyclomatic: 25 }), 0)).toBe(0);
    // architecture: violation sources severe; overview always none.
    const vIndex = buildIndex(
      data({
        violations: [{ from: 0, to: 2, from_zone: 0, to_zone: 1, line: 3, specifier: "x" }],
      }),
    );
    expect(lensFindingLevel("architecture", vIndex, file(), 0)).toBe(2);
    expect(lensFindingLevel("architecture", vIndex, file(), 1)).toBe(0);
    expect(lensFindingLevel("overview", vIndex, file({ status: "unused" }), 0)).toBe(0);
  });
});

describe("formatting", () => {
  it("scales byte sizes", () => {
    expect(formatSize(512)).toBe("512 B");
    expect(formatSize(2048)).toBe("2.0 KB");
  });

  it("splits paths", () => {
    expect(basename("a/b/c.ts")).toBe("c.ts");
    expect(dirname("a/b/c.ts")).toBe("a/b");
    expect(dirname("c.ts")).toBe("");
  });
});

describe("reachSet", () => {
  // Chain a -> b -> c: c is reached from a downstream; a affects c upstream.
  const adjDown = [[1], [2], []]; // importsOf: a imports b, b imports c
  const adjUp = [[], [0], [1]]; // importersOf: b imported by a, c imported by b

  it("collects the full transitive downstream set, excluding the start", () => {
    const r = reachSet(adjDown, 0);
    expect([...r].toSorted()).toEqual([1, 2]);
    expect(r.has(0)).toBe(false);
  });

  it("collects the full transitive upstream (blast radius) set", () => {
    expect([...reachSet(adjUp, 2)].toSorted()).toEqual([0, 1]);
    expect(reachSet(adjUp, 0).size).toBe(0);
  });

  it("terminates on a cycle instead of looping forever", () => {
    const cyclic = [[1], [0]];
    expect([...reachSet(cyclic, 0)].toSorted()).toEqual([1]);
  });
});

describe("reachSetMulti", () => {
  // Diamond: 3 and 4 both import 1 and 2; 1 and 2 both import 0.
  const adjUp = [[1, 2], [3, 4], [3, 4], [], []]; // importersOf
  const seeds = [1, 2];

  it("matches the union of the per-seed reachSet, minus the seeds", () => {
    const union = new Set([...reachSet(adjUp, 1), ...reachSet(adjUp, 2)]);
    for (const s of seeds) union.delete(s);
    expect([...reachSetMulti(adjUp, seeds)].toSorted()).toEqual([...union].toSorted());
    expect([...reachSetMulti(adjUp, seeds)].toSorted()).toEqual([3, 4]);
  });

  it("excludes every seed even when one seed is reachable from another", () => {
    // 1 imports 0, and 0 is also a seed: 0 must not appear in the reach.
    const adj = [[1], [], []]; // importersOf: 0 imported by 1
    expect([...reachSetMulti(adj, [0, 1])].toSorted()).toEqual([]);
  });

  it("returns an empty set for empty seeds", () => {
    expect(reachSetMulti(adjUp, []).size).toBe(0);
  });
});

describe("runSearch combined blast radius", () => {
  it("collects the union upstream reach of every matched file", () => {
    // a-alpha.ts imported by b, which is imported by c: searching "alpha"
    // should mark b and c as affected.
    const d = data({
      files: [
        file({ path: "src/alpha.ts" }),
        file({ path: "src/b.ts" }),
        file({ path: "src/c.ts" }),
      ],
      edges: [
        [1, 0, 0],
        [2, 1, 0],
      ],
    });
    const state = {
      data: d,
      index: buildIndex(d),
      search: "",
      searchMatches: new Set<number>(),
      searchReach: new Set<number>(),
    } as unknown as AppState;
    runSearch(state, "alpha");
    expect(state.searchMatches.has(0)).toBe(true);
    expect([...state.searchReach].toSorted()).toEqual([1, 2]);
    expect(state.searchReach.has(0)).toBe(false);
  });

  it("computes the blast radius even for match sets past the old 40-file cap", () => {
    // 50 matches (over the retired cap) all imported by one consumer: the
    // multi-source traversal must still surface that consumer.
    const widgets = Array.from({ length: 50 }, (_, i) => file({ path: `src/widget-${i}.ts` }));
    const files = [...widgets, file({ path: "src/app.ts" })];
    const consumer = files.length - 1;
    const edges: [number, number, number][] = widgets.map((_, i) => [consumer, i, 0]);
    const d = data({ files, edges });
    const state = {
      data: d,
      index: buildIndex(d),
      search: "",
      searchMatches: new Set<number>(),
      searchReach: new Set<number>(),
    } as unknown as AppState;
    runSearch(state, "widget");
    expect(state.searchMatches.size).toBe(50);
    expect(state.searchReach.has(consumer)).toBe(true);
  });
});
