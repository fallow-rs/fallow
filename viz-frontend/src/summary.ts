import type { VizSummary } from "./types";
import type { ColorTheme } from "./colors";
import { formatSize } from "./utils";

export const createSummaryBar = (
  summary: VizSummary,
  theme: ColorTheme,
): HTMLElement => {
  const bar = document.createElement("div");
  bar.id = "fallow-summary";
  bar.style.backgroundColor = theme.summaryBg;

  const items: Array<{ label: string; value: string; color?: string }> = [
    { label: "Files", value: String(summary.total_files) },
    { label: "Size", value: formatSize(summary.total_size) },
  ];

  if (summary.unused_files > 0) {
    items.push({
      label: "Unused files",
      value: String(summary.unused_files),
      color: theme.statusColors.unused,
    });
  }

  if (summary.unused_exports > 0) {
    items.push({
      label: "Unused exports",
      value: String(summary.unused_exports),
      color: theme.statusColors.hasUnusedExports,
    });
  }

  if (summary.unused_deps > 0) {
    items.push({ label: "Unused deps", value: String(summary.unused_deps) });
  }

  if (summary.unresolved_imports > 0) {
    items.push({ label: "Unresolved", value: String(summary.unresolved_imports) });
  }

  if (summary.circular_deps > 0) {
    items.push({ label: "Cycles", value: String(summary.circular_deps) });
  }

  for (const item of items) {
    const el = document.createElement("span");
    el.className = "summary-item";

    const label = document.createElement("span");
    label.className = "summary-label";
    label.textContent = item.label;
    el.appendChild(label);

    const value = document.createElement("span");
    value.className = "summary-value";
    value.textContent = item.value;
    if (item.color) value.style.color = item.color;
    el.appendChild(value);

    bar.appendChild(el);
  }

  return bar;
};
