import type { AppState } from "./state";
import { basename, dirname, dupRatio, formatCount, formatSize } from "./data";

let tipEl: HTMLDivElement | null = null;

const getTip = (): HTMLDivElement => {
  if (!tipEl) {
    tipEl = document.createElement("div");
    tipEl.id = "tooltip";
    document.body.appendChild(tipEl);
  }
  return tipEl;
};

const line = (cls: string, text: string): HTMLElement => {
  const div = document.createElement("div");
  div.className = cls;
  div.textContent = text;
  return div;
};

interface Stat {
  value: string;
  label: string;
  /** Severity class on the value ("sev-error" | "sev-warn"). */
  cls?: string;
}

/** Mini stat row: prominent value over a muted micro-label per fact. */
const statGrid = (stats: Stat[]): HTMLElement => {
  const grid = document.createElement("div");
  grid.className = "tip-stats";
  for (const stat of stats) {
    const tile = document.createElement("div");
    tile.className = "tip-stat";
    const valueEl = document.createElement("div");
    valueEl.className = stat.cls ? `v ${stat.cls}` : "v";
    valueEl.textContent = stat.value;
    const labelEl = document.createElement("div");
    labelEl.className = "l";
    labelEl.textContent = stat.label;
    tile.append(valueEl, labelEl);
    grid.appendChild(tile);
  }
  return grid;
};

/** Fixed tooltip width; the label exclusion zone mirrors it. */
const TIP_W = 260;
/** Height estimate for the exclusion zone (real height varies a little). */
const TIP_EST_H = 150;

/** Edge-docked placement in canvas space (mirrored by graph.ts labels). */
export const fileTipCanvasRect = (
  nodeX: number,
  nodeY: number,
  canvasW: number,
  canvasH: number,
): { x: number; y: number; w: number; h: number } => ({
  x: nodeX < canvasW / 2 ? canvasW - TIP_W - 12 : 12,
  y: Math.min(Math.max(nodeY - 60, 12), Math.max(12, canvasH - TIP_EST_H - 12)),
  w: TIP_W,
  h: TIP_EST_H,
});

/** Optional docking info for graph hover: pin to the far canvas edge. */
export interface TipDock {
  nodeX: number;
  nodeY: number;
  canvas: DOMRect;
  /** Width not covered by an open panel; the dock clamps within it. */
  usableW: number;
}

/** The single worst thing to say about a file (one line, or nothing). */
const severityLine = (state: AppState, fileIndex: number): HTMLElement | null => {
  const file = state.data.files[fileIndex];
  if (file.status === "unused") {
    return line("sev-error tip-line", "Unused, nothing imports this file");
  }
  if (state.index.violationSources.has(fileIndex)) {
    const count = state.data.violations.filter((violation) => violation.from === fileIndex).length;
    return line(
      "sev-error tip-line",
      `${formatCount(count)} boundary violation${count === 1 ? "" : "s"}`,
    );
  }
  if (file.in_cycle) return line("sev-warn tip-line", "Part of a dependency cycle");
  if (file.unused_export_count > 0) {
    return line(
      "sev-warn tip-line",
      `${formatCount(file.unused_export_count)} unused export${file.unused_export_count === 1 ? "" : "s"}`,
    );
  }
  if (file.status === "entryPoint") return line("sev-info tip-line", "Entry point");
  return null;
};

/** One extra line only when the active lens has something to add. */
const lensLine = (state: AppState, fileIndex: number): HTMLElement | null => {
  const file = state.data.files[fileIndex];
  if (state.lens === "dupes" && file.dup_lines > 0) {
    return line(
      "sev-warn tip-line",
      `${formatCount(file.dup_lines)} duplicated lines, ${Math.round(dupRatio(file) * 100)}% of the file`,
    );
  }
  if (state.lens === "hotspots" && file.max_cyclomatic > 0) {
    const cls =
      file.max_cyclomatic >= 20
        ? "sev-error"
        : file.max_cyclomatic >= 10
          ? "sev-warn"
          : "tip-muted";
    return line(
      `${cls} tip-line`,
      `Complexity ${formatCount(file.max_cyclomatic)}, nesting ${formatCount(file.max_cognitive)}`,
    );
  }
  if (state.lens === "boundaries" && file.zone !== undefined) {
    const zone = state.data.zones[file.zone]?.name;
    return zone ? line("tip-muted tip-line", `Zone ${zone}`) : null;
  }
  return null;
};

/**
 * Show the tooltip for a file. Hover is the lightweight preview: name,
 * wiring counts, and at most two single-line qualifiers; everything
 * else waits for the click. With `dock` (graph view) the tooltip pins
 * to the canvas edge opposite the node, so it never covers the
 * neighborhood it describes.
 */
export const showFileTooltip = (
  state: AppState,
  fileIndex: number,
  mouseX: number,
  mouseY: number,
  dock?: TipDock,
): void => {
  const file = state.data.files[fileIndex];
  const tip = getTip();
  tip.replaceChildren();

  const dir = dirname(file.path);
  if (dir) tip.appendChild(line("tip-dir", `${dir}/`));
  const nameRow = line("tip-name-row", "");
  nameRow.appendChild(line("tip-name", basename(file.path)));
  nameRow.appendChild(line("tip-size", formatSize(file.size)));
  tip.appendChild(nameRow);

  // Wiring counts, prefixed like the hover edges: ● solid = importers,
  // ○ outline = its imports.
  const wires = line("tip-wires", "");
  const importers = document.createElement("span");
  importers.appendChild(line("wire-dot", "●"));
  importers.appendChild(document.createTextNode(` ${formatCount(file.importer_count)} importers`));
  wires.appendChild(importers);
  const imports = document.createElement("span");
  imports.appendChild(line("wire-dot", "○"));
  imports.appendChild(document.createTextNode(` ${formatCount(file.import_count)} imports`));
  wires.appendChild(imports);
  tip.appendChild(wires);

  const sev1 = severityLine(state, fileIndex);
  if (sev1) tip.appendChild(sev1);
  const lens1 = lensLine(state, fileIndex);
  if (lens1) tip.appendChild(lens1);
  // No generic "click for details" line: the pointer cursor already says
  // the node is clickable, and the severity/lens lines carry the substance.

  if (dock) {
    tip.style.display = "block";
    const rect = fileTipCanvasRect(dock.nodeX, dock.nodeY, dock.usableW, dock.canvas.height);
    const height = tip.getBoundingClientRect().height;
    tip.style.left = `${dock.canvas.left + rect.x}px`;
    tip.style.top = `${Math.min(dock.canvas.top + rect.y, dock.canvas.bottom - height - 12)}px`;
  } else {
    position(tip, mouseX, mouseY);
  }
};

/** Show the tooltip for an aggregated road (graph overview). */
export const showRoadTooltip = (
  srcKey: string,
  dstKey: string,
  count: number,
  violations: number,
  cycleEdges: number,
  mouseX: number,
  mouseY: number,
): void => {
  const tip = getTip();
  tip.replaceChildren();
  tip.appendChild(line("tip-kind", "Imports"));
  tip.appendChild(line("tip-name", `${srcKey} → ${dstKey}`));
  const stats: Array<{ value: string; label: string; cls?: string }> = [
    { value: formatCount(count), label: "imports" },
  ];
  if (violations > 0) {
    stats.push({ value: formatCount(violations), label: "violations", cls: "sev-error" });
  }
  if (cycleEdges > 0) {
    stats.push({ value: formatCount(cycleEdges), label: "cycle edges", cls: "sev-warn" });
  }
  tip.appendChild(statGrid(stats));
  tip.appendChild(line("tip-muted tip-line", "Click to list every import"));
  position(tip, mouseX, mouseY);
};

/** Show the tooltip for a treemap directory cell. */
export const showDirTooltip = (
  name: string,
  fileCount: number,
  size: number,
  findings: { value: number; label: string } | null,
  mouseX: number,
  mouseY: number,
): void => {
  const tip = getTip();
  tip.replaceChildren();
  tip.appendChild(line("tip-kind", "Folder"));
  tip.appendChild(line("tip-name", `${name}/`));
  const stats: Stat[] = [
    { value: formatCount(fileCount), label: "files" },
    { value: formatSize(size), label: "size" },
  ];
  if (findings && findings.value > 0) {
    stats.push({
      value: formatCount(findings.value),
      label: findings.label,
      cls: "sev-warn",
    });
  }
  tip.appendChild(statGrid(stats));
  tip.appendChild(line("tip-muted tip-line", "Click to zoom in"));
  position(tip, mouseX, mouseY);
};

const position = (tip: HTMLDivElement, mouseX: number, mouseY: number): void => {
  tip.style.display = "block";
  const rect = tip.getBoundingClientRect();
  let left = mouseX + 14;
  let top = mouseY + 14;
  if (left + rect.width > window.innerWidth - 12) left = mouseX - rect.width - 14;
  if (top + rect.height > window.innerHeight - 12) top = mouseY - rect.height - 14;
  tip.style.left = `${Math.max(12, left)}px`;
  tip.style.top = `${Math.max(12, top)}px`;
};

export const hideTooltip = (): void => {
  if (tipEl) tipEl.style.display = "none";
};
