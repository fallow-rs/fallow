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
    const v = document.createElement("div");
    v.className = stat.cls ? `v ${stat.cls}` : "v";
    v.textContent = stat.value;
    const l = document.createElement("div");
    l.className = "l";
    l.textContent = stat.label;
    tile.append(v, l);
    grid.appendChild(tile);
  }
  return grid;
};

const header = (tip: HTMLDivElement, path: string): void => {
  const dir = dirname(path);
  if (dir) tip.appendChild(line("tip-dir", `${dir}/`));
  tip.appendChild(line("tip-name", basename(path)));
};

/** Show the tooltip for a file (both views route through here). */
export const showFileTooltip = (
  state: AppState,
  fileIndex: number,
  mouseX: number,
  mouseY: number,
): void => {
  const file = state.data.files[fileIndex];
  const tip = getTip();
  tip.replaceChildren();

  header(tip, file.path);

  const facts: Stat[] = [{ value: formatSize(file.size), label: "size" }];
  if (file.export_count > 0) {
    facts.push({ value: formatCount(file.export_count), label: "exports" });
  }
  facts.push({ value: formatCount(file.importer_count), label: "imported by" });
  if (file.import_count > 0) {
    facts.push({ value: formatCount(file.import_count), label: "imports" });
  }
  tip.appendChild(statGrid(facts));

  switch (file.status) {
    case "unused":
      tip.appendChild(
        line(
          "sev-error tip-line",
          file.importer_count === 0 ? "unused file, nothing imports it" : "unused file",
        ),
      );
      break;
    case "hasUnusedExports": {
      const names = file.unused_exports ?? [];
      const shown = names.slice(0, 5).join(", ");
      const extra = names.length > 5 ? ` +${names.length - 5}` : "";
      tip.appendChild(line("sev-warn tip-line", `unused: ${shown}${extra}`));
      break;
    }
    case "entryPoint":
      tip.appendChild(line("sev-info tip-line", "entry point"));
      break;
    default:
      break;
  }

  if (state.lens === "dupes" && file.dup_lines > 0) {
    tip.appendChild(
      statGrid([
        { value: formatCount(file.dup_lines), label: "dup lines", cls: "sev-warn" },
        { value: `${Math.round(dupRatio(file) * 100)}%`, label: "of file" },
        { value: formatCount(file.clone_groups?.length ?? 0), label: "groups" },
      ]),
    );
  }
  if (state.lens === "hotspots" && file.max_cyclomatic > 0) {
    const ccCls =
      file.max_cyclomatic >= 20 ? "sev-error" : file.max_cyclomatic >= 10 ? "sev-warn" : undefined;
    const cogCls =
      file.max_cognitive >= 25 ? "sev-error" : file.max_cognitive >= 15 ? "sev-warn" : undefined;
    const heat: Stat[] = [
      { value: String(file.max_cyclomatic), label: "cyclomatic", cls: ccCls },
      { value: String(file.max_cognitive), label: "cognitive", cls: cogCls },
    ];
    if (file.react_hooks > 0) heat.push({ value: String(file.react_hooks), label: "hooks" });
    if (file.jsx_depth > 0) heat.push({ value: String(file.jsx_depth), label: "jsx depth" });
    tip.appendChild(statGrid(heat));
    const top = file.functions?.[0];
    if (top && ccCls) {
      const worst = line("tip-line", "");
      const label = document.createElement("span");
      label.className = "tip-muted";
      label.textContent = "worst ";
      const fn = document.createElement("span");
      fn.className = "fn";
      fn.textContent = `${top.name}()`;
      worst.append(label, fn);
      tip.appendChild(worst);
    }
  }
  if (state.lens === "boundaries") {
    const zone = file.zone !== undefined ? state.data.zones[file.zone]?.name : undefined;
    const zoneLine = line("tip-line", "");
    const label = document.createElement("span");
    label.className = "tip-muted";
    label.textContent = "zone ";
    zoneLine.appendChild(label);
    const value = document.createElement("span");
    value.className = zone ? "sev-info" : "tip-muted";
    value.textContent = zone ?? "none";
    zoneLine.appendChild(value);
    tip.appendChild(zoneLine);
    if (state.index.violationSources.has(fileIndex)) {
      const count = state.data.violations.filter((v) => v.from === fileIndex).length;
      tip.appendChild(
        line("sev-error tip-line", `${count} boundary violation${count === 1 ? "" : "s"}`),
      );
    }
  }
  if (file.in_cycle) {
    tip.appendChild(line("sev-warn tip-line", "part of a dependency cycle"));
  }

  tip.appendChild(line("tip-muted tip-line", "click for details"));
  position(tip, mouseX, mouseY);
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
  tip.appendChild(line("tip-name", `${srcKey} ▸ ${dstKey}`));
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
  tip.appendChild(line("tip-muted tip-line", "click to list the file pairs"));
  position(tip, mouseX, mouseY);
};

/** Show the tooltip for a treemap directory cell. */
export const showDirTooltip = (
  name: string,
  fileCount: number,
  size: number,
  mouseX: number,
  mouseY: number,
): void => {
  const tip = getTip();
  tip.replaceChildren();
  tip.appendChild(line("tip-name", `${name}/`));
  tip.appendChild(
    statGrid([
      { value: formatCount(fileCount), label: "files" },
      { value: formatSize(size), label: "size" },
    ]),
  );
  tip.appendChild(line("tip-muted tip-line", "click to zoom in"));
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
