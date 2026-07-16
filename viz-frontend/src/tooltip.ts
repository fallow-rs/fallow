import type { AppState } from "./state";
import { dupRatio, formatCount, formatSize } from "./data";

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

  tip.appendChild(line("tip-path", file.path));

  const facts: string[] = [formatSize(file.size)];
  if (file.export_count > 0) facts.push(`${formatCount(file.export_count)} exports`);
  if (file.importer_count > 0) facts.push(`${formatCount(file.importer_count)} importers`);
  if (file.import_count > 0) facts.push(`${formatCount(file.import_count)} imports`);
  tip.appendChild(line("tip-dim", facts.join(" · ")));

  switch (file.status) {
    case "unused":
      tip.appendChild(
        line(
          "sev-error",
          file.importer_count === 0 ? "[E] unused file · nothing imports it" : "[E] unused file",
        ),
      );
      break;
    case "hasUnusedExports": {
      const names = file.unused_exports ?? [];
      const shown = names.slice(0, 5).join(", ");
      const extra = names.length > 5 ? ` +${names.length - 5}` : "";
      tip.appendChild(line("sev-warn", `[W] unused: ${shown}${extra}`));
      break;
    }
    case "entryPoint":
      tip.appendChild(line("sev-info", "[*] entry point"));
      break;
    default:
      break;
  }

  if (state.lens === "dupes" && file.dup_lines > 0) {
    tip.appendChild(
      line(
        "sev-warn",
        `[W] ${formatCount(file.dup_lines)} duplicated lines (${Math.round(dupRatio(file) * 100)}%) · ${file.clone_groups?.length ?? 0} groups`,
      ),
    );
  }
  if (state.lens === "hotspots" && file.max_cyclomatic > 0) {
    const cls =
      file.max_cyclomatic >= 20 ? "sev-error" : file.max_cyclomatic >= 10 ? "sev-warn" : "tip-dim";
    const top = file.functions?.[0];
    tip.appendChild(
      line(
        cls,
        `cc ${file.max_cyclomatic} · cog ${file.max_cognitive}${top ? ` · worst: ${top.name}()` : ""}`,
      ),
    );
    if (file.react_hooks > 0 || file.jsx_depth > 0) {
      tip.appendChild(
        line("tip-muted", `react: ${file.react_hooks} hooks · jsx depth ${file.jsx_depth}`),
      );
    }
  }
  if (state.lens === "boundaries") {
    const zone = file.zone !== undefined ? state.data.zones[file.zone]?.name : undefined;
    tip.appendChild(line(zone ? "sev-info" : "tip-muted", zone ? `zone: ${zone}` : "no zone"));
    if (state.index.violationSources.has(fileIndex)) {
      const count = state.data.violations.filter((v) => v.from === fileIndex).length;
      tip.appendChild(
        line("sev-error", `[E] ${count} boundary violation${count === 1 ? "" : "s"}`),
      );
    }
  }
  if (file.in_cycle) {
    tip.appendChild(line("sev-warn", "[W] part of a dependency cycle"));
  }

  tip.appendChild(line("tip-muted", "click for details"));
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
  tip.appendChild(line("tip-path", `${name}/`));
  tip.appendChild(line("tip-dim", `${formatCount(fileCount)} files · ${formatSize(size)}`));
  tip.appendChild(line("tip-muted", "click to zoom in"));
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
