import type { VizData, LayoutNode } from "./types";
import type { ColorTheme } from "./colors";
import { formatSize } from "./utils";

let tooltipEl: HTMLDivElement | null = null;

const getTooltip = (): HTMLDivElement => {
  if (!tooltipEl) {
    tooltipEl = document.createElement("div");
    tooltipEl.id = "fallow-tooltip";
    document.body.appendChild(tooltipEl);
  }
  return tooltipEl;
};

const STATUS_LABELS: Record<string, string> = {
  clean: "Clean",
  hasUnusedExports: "Has unused exports",
  unused: "Unused file",
  entryPoint: "Entry point",
};

const createEl = (
  tag: string,
  className: string,
  text: string,
  style?: string,
): HTMLElement => {
  const el = document.createElement(tag);
  el.className = className;
  el.textContent = text;
  if (style) el.setAttribute("style", style);
  return el;
};

export const showTooltip = (
  cell: LayoutNode,
  data: VizData,
  theme: ColorTheme,
  mouseX: number,
  mouseY: number,
): void => {
  const tip = getTooltip();
  const isFile = cell.node.fileIndex !== null;

  // Clear previous content safely
  tip.replaceChildren();

  tip.appendChild(createEl("div", "tip-path", cell.node.path));
  tip.appendChild(createEl("div", "tip-size", formatSize(cell.node.size)));

  if (isFile) {
    const file = data.files[cell.node.fileIndex!];
    const statusLabel = STATUS_LABELS[file.status] ?? file.status;
    const statusColor = theme.statusColors[file.status];

    tip.appendChild(
      createEl("div", "tip-status", statusLabel, `color:${statusColor}`),
    );

    const details: string[] = [];
    if (file.export_count > 0) {
      details.push(`${file.export_count} export${file.export_count !== 1 ? "s" : ""}`);
    }
    if (file.import_count > 0) {
      details.push(`${file.import_count} import${file.import_count !== 1 ? "s" : ""}`);
    }
    if (file.importer_count > 0) {
      details.push(`${file.importer_count} importer${file.importer_count !== 1 ? "s" : ""}`);
    }
    if (details.length > 0) {
      tip.appendChild(createEl("div", "tip-details", details.join(" · ")));
    }

    // Show actual unused export names, the actionable part
    if (file.unused_exports && file.unused_exports.length > 0) {
      const maxShow = 8;
      const names = file.unused_exports.slice(0, maxShow);
      const label = names.map((n) => n).join(", ");
      const suffix = file.unused_exports.length > maxShow
        ? ` (+${file.unused_exports.length - maxShow} more)`
        : "";
      tip.appendChild(
        createEl("div", "tip-unused", `Unused: ${label}${suffix}`, `color:${theme.statusColors.hasUnusedExports}`),
      );
    }

    if (file.workspace) {
      tip.appendChild(createEl("div", "tip-workspace", file.workspace));
    }
  } else {
    tip.appendChild(
      createEl("div", "tip-status", "Directory, click to expand", `color:${theme.textSecondary}`),
    );
  }

  tip.style.backgroundColor = theme.tooltipBg;
  tip.style.color = theme.tooltipText;
  tip.style.borderColor = theme.tooltipBorder;
  tip.style.display = "block";

  // Position near cursor, flip if near edge
  const padding = 12;
  const tipRect = tip.getBoundingClientRect();
  let left = mouseX + padding;
  let top = mouseY + padding;

  if (left + tipRect.width > window.innerWidth - padding) {
    left = mouseX - tipRect.width - padding;
  }
  if (top + tipRect.height > window.innerHeight - padding) {
    top = mouseY - tipRect.height - padding;
  }

  tip.style.left = `${Math.max(padding, left)}px`;
  tip.style.top = `${Math.max(padding, top)}px`;
};

export const hideTooltip = (): void => {
  if (tooltipEl) {
    tooltipEl.style.display = "none";
  }
};
