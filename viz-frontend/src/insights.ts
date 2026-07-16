import type { ActiveView, Lens, VizData } from "./types";
import type { DataIndex } from "./data";
import { basename, formatCount } from "./data";

/**
 * Auto-generated orientation insights: the handful of facts someone
 * should see first on this codebase, each with a click-through action.
 * Pure derivation from the embedded payload; feeds the briefing drawer
 * and the guided tour.
 */

export type InsightAction =
  | { kind: "file"; fileIndex: number; lens?: Lens; view?: ActiveView }
  | { kind: "dir"; path: string; lens?: Lens }
  | { kind: "road"; srcDir: string; dstDir: string }
  | { kind: "lens"; lens: Lens; view?: ActiveView };

export interface Insight {
  severity: "error" | "warn" | "info";
  /** One-line headline. */
  text: string;
  /** Optional secondary line. */
  detail?: string;
  action: InsightAction;
}

const topDir = (path: string): string => path.split("/")[0];

export const computeInsights = (data: VizData, index: DataIndex): Insight[] => {
  const insights: Insight[] = [];
  const files = data.files;
  const s = data.summary;

  // 1. Unused files: largest one + concentration directory.
  if (s.unused_files > 0) {
    const unused = files
      .map((f, i) => ({ f, i }))
      .filter(({ f }) => f.status === "unused");
    const largest = unused.reduce((a, b) => (b.f.size > a.f.size ? b : a));
    const byDir = new Map<string, number>();
    for (const { f } of unused) byDir.set(topDir(f.path), (byDir.get(topDir(f.path)) ?? 0) + 1);
    const [hotDir, hotCount] = [...byDir.entries()].sort((a, b) => b[1] - a[1])[0];
    insights.push({
      severity: "error",
      text: `${formatCount(s.unused_files)} unused file${s.unused_files === 1 ? "" : "s"}, nothing imports them`,
      detail:
        hotCount >= 3
          ? `${hotCount} of them under ${hotDir}/ · largest is ${basename(largest.f.path)}`
          : `largest is ${basename(largest.f.path)}`,
      action: { kind: "file", fileIndex: largest.i, lens: "deadcode" },
    });
  }

  // 2. Boundary violations.
  if (s.boundary_violations > 0 && data.violations.length > 0) {
    const v = data.violations[0];
    insights.push({
      severity: "error",
      text: `${formatCount(s.boundary_violations)} import${s.boundary_violations === 1 ? "" : "s"} cross a forbidden boundary`,
      detail: `${data.zones[v.from_zone]?.name ?? "?"} is not allowed to import ${data.zones[v.to_zone]?.name ?? "?"}`,
      action: { kind: "file", fileIndex: v.from, lens: "boundaries", view: "graph" },
    });
  }

  // 3. Circular dependencies.
  if (s.circular_deps > 0 && data.cycles.length > 0) {
    const longest = data.cycles.reduce((a, b) => (b.length > a.length ? b : a));
    insights.push({
      severity: "warn",
      text: `${formatCount(s.circular_deps)} circular ${s.circular_deps === 1 ? "dependency" : "dependencies"}`,
      detail: `longest cycle spans ${longest.length} files around ${basename(files[longest[0]].path)}`,
      action: { kind: "file", fileIndex: longest[0], lens: "deadcode", view: "graph" },
    });
  }

  // 4. Heaviest clone group.
  if (data.clones.length > 0) {
    const heaviest = data.clones.reduce((a, b) =>
      b.lines * b.instances.length > a.lines * a.instances.length ? b : a,
    );
    const first = heaviest.instances[0];
    if (first) {
      insights.push({
        severity: "warn",
        text: `${formatCount(s.clone_groups)} clone group${s.clone_groups === 1 ? "" : "s"}, ${formatCount(s.duplicated_lines)} duplicated lines`,
        detail: `largest block: ${heaviest.lines} lines in ${heaviest.instances.length} places`,
        action: { kind: "file", fileIndex: first.file, lens: "dupes" },
      });
    }
  }

  // 5. Worst complexity hotspot.
  const hotspot = files
    .map((f, i) => ({ f, i }))
    .filter(({ f }) => f.max_cyclomatic > 0)
    .sort((a, b) => b.f.max_cyclomatic - a.f.max_cyclomatic)[0];
  if (hotspot && hotspot.f.max_cyclomatic >= 10) {
    const fn = hotspot.f.functions?.[0];
    insights.push({
      severity: "warn",
      text: `${formatCount(s.hotspot_files)} complexity hotspot${s.hotspot_files === 1 ? "" : "s"}`,
      detail: fn
        ? `worst: ${fn.name}() in ${basename(hotspot.f.path)} (cc ${fn.cyclomatic})`
        : `worst: ${basename(hotspot.f.path)} (cc ${hotspot.f.max_cyclomatic})`,
      action: { kind: "file", fileIndex: hotspot.i, lens: "hotspots" },
    });
  }

  // 6. Most depended-on file (hub).
  const hub = files
    .map((f, i) => ({ f, i }))
    .sort((a, b) => b.f.importer_count - a.f.importer_count)[0];
  if (hub && hub.f.importer_count >= 10) {
    insights.push({
      severity: "info",
      text: `${basename(hub.f.path)} is imported by ${formatCount(hub.f.importer_count)} files`,
      detail: "a change here ripples through the whole map",
      action: { kind: "file", fileIndex: hub.i, view: "graph" },
    });
  }

  // 7. Heaviest cross-directory dependency.
  const roadCounts = new Map<string, { src: string; dst: string; count: number }>();
  for (const [from, to] of data.edges) {
    const src = topDir(files[from].path);
    const dst = topDir(files[to].path);
    if (src === dst) continue;
    const key = `${src}=>${dst}`;
    const entry = roadCounts.get(key) ?? { src, dst, count: 0 };
    entry.count++;
    roadCounts.set(key, entry);
  }
  const heaviestRoad = [...roadCounts.values()].sort((a, b) => b.count - a.count)[0];
  if (heaviestRoad && heaviestRoad.count >= 10) {
    insights.push({
      severity: "info",
      text: `heaviest dependency: ${heaviestRoad.src} ▸ ${heaviestRoad.dst}`,
      detail: `${formatCount(heaviestRoad.count)} imports flow along this road`,
      action: { kind: "road", srcDir: heaviestRoad.src, dstDir: heaviestRoad.dst },
    });
  }

  // Order: errors, then warns, then info; cap at 6.
  const rank = { error: 0, warn: 1, info: 2 };
  insights.sort((a, b) => rank[a.severity] - rank[b.severity]);
  void index;
  return insights.slice(0, 6);
};
