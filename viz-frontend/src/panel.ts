import type { AppState } from "./state";
import type { VizFile } from "./types";
import { basename, dirname, formatCount, formatSize } from "./data";

/** Called when the user clicks through to another file. */
export type NavigateFn = (fileIndex: number) => void;

const el = (tag: string, cls?: string, text?: string): HTMLElement => {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
};

const sectionEl = (title: string): HTMLElement => {
  const section = el("section");
  const h = el("h3");
  const open = el("span", "bracket", "[ ");
  const label = document.createTextNode(title);
  const close = el("span", "bracket", " ]");
  h.append(open, label, close);
  section.appendChild(h);
  return section;
};

const kvEl = (pairs: Array<[string, string | HTMLElement]>): HTMLElement => {
  const dl = el("dl", "kv");
  for (const [k, v] of pairs) {
    const dt = el("dt", undefined, k);
    const dd = el("dd");
    if (typeof v === "string") dd.textContent = v;
    else dd.appendChild(v);
    dl.append(dt, dd);
  }
  return dl;
};

const sev = (cls: string, text: string): HTMLElement => el("span", cls, text);

/** ASCII severity bar: `████░░░░` colored by threshold. */
const asciiBar = (value: number, max: number, dangerAt: number): HTMLElement => {
  const slots = 8;
  const filled = Math.max(0, Math.min(slots, Math.round((value / max) * slots)));
  const bar = el("span", "bar");
  const fill = el(
    "span",
    value >= dangerAt ? "fill-error" : "fill-warn",
    "█".repeat(filled),
  );
  bar.append(fill, document.createTextNode("░".repeat(slots - filled)));
  return bar;
};

const statusLabel = (file: VizFile): HTMLElement => {
  const wrap = el("span");
  switch (file.status) {
    case "unused":
      wrap.appendChild(sev("sev-error", "unused file"));
      break;
    case "hasUnusedExports":
      wrap.appendChild(
        sev("sev-warn", `${formatCount(file.unused_export_count)} unused export${file.unused_export_count === 1 ? "" : "s"}`),
      );
      break;
    case "entryPoint":
      wrap.appendChild(sev("sev-info", "entry point"));
      break;
    default:
      wrap.appendChild(sev("sev-ok", "live"));
  }
  return wrap;
};

const linkList = (
  state: AppState,
  indices: number[],
  navigate: NavigateFn,
  cap = 30,
): HTMLElement => {
  const ul = el("ul", "link-list");
  for (const idx of indices.slice(0, cap)) {
    const li = el("li");
    const btn = el("button", undefined, state.data.files[idx].path) as HTMLButtonElement;
    btn.type = "button";
    btn.addEventListener("click", () => navigate(idx));
    li.appendChild(btn);
    ul.appendChild(li);
  }
  if (indices.length > cap) {
    ul.appendChild(el("li", "muted", `… ${formatCount(indices.length - cap)} more`));
  }
  return ul;
};

export const createPanel = (): HTMLElement => {
  const panel = el("aside");
  panel.id = "panel";
  panel.setAttribute("aria-label", "File details");
  return panel;
};

export const renderPanel = (
  state: AppState,
  panel: HTMLElement,
  navigate: NavigateFn,
  close: () => void,
): void => {
  if (state.selected === null && state.selectedRoad !== null) {
    renderRoadPanel(state, panel, navigate, close);
    return;
  }
  if (state.selected === null) {
    panel.classList.remove("open");
    return;
  }

  const fileIdx = state.selected;
  const file = state.data.files[fileIdx];
  panel.replaceChildren();
  panel.classList.add("open");

  // Header
  const head = el("div", "panel-head");
  const fileBox = el("div", "file");
  const dir = dirname(file.path);
  if (dir) fileBox.appendChild(el("div", "dir", `${dir}/`));
  fileBox.appendChild(el("div", "name", basename(file.path)));
  const statusLine = el("div", "status-line");
  statusLine.appendChild(statusLabel(file));
  fileBox.appendChild(statusLine);
  head.appendChild(fileBox);
  const closeBtn = el("button", "close", "×") as HTMLButtonElement;
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close details");
  closeBtn.addEventListener("click", close);
  head.appendChild(closeBtn);
  panel.appendChild(head);

  // Facts
  const facts = sectionEl("facts");
  const pairs: Array<[string, string | HTMLElement]> = [
    ["size", formatSize(file.size)],
    ["exports", formatCount(file.export_count)],
    ["imports", formatCount(file.import_count)],
    ["imported by", formatCount(file.importer_count)],
  ];
  if (file.workspace !== undefined && state.data.workspaces[file.workspace]) {
    pairs.push(["workspace", state.data.workspaces[file.workspace].name]);
  }
  if (file.zone !== undefined && state.data.zones[file.zone]) {
    pairs.push(["zone", state.data.zones[file.zone].name]);
  }
  if (file.fn_count > 0) pairs.push(["functions", formatCount(file.fn_count)]);
  facts.appendChild(kvEl(pairs));
  panel.appendChild(facts);

  // Dead code
  if (file.status === "unused") {
    const dead = sectionEl("dead code");
    const msg = el("div", "sev-error");
    msg.textContent =
      file.importer_count === 0
        ? "no file imports this one; nothing reaches it from an entry point."
        : "unreachable from every entry point.";
    dead.appendChild(msg);
    const hint = el("div", "action-hint");
    hint.append("verify: ", elCode(`fallow dead-code --trace ${file.path}`));
    dead.appendChild(hint);
    panel.appendChild(dead);
  } else if (file.unused_exports && file.unused_exports.length > 0) {
    const dead = sectionEl("unused exports");
    const tags = el("div", "tag-list");
    for (const name of file.unused_exports.slice(0, 20)) {
      tags.appendChild(el("span", "tag", name));
    }
    if (file.unused_exports.length > 20) {
      tags.appendChild(el("span", "muted", `… ${file.unused_exports.length - 20} more`));
    }
    dead.appendChild(tags);
    const hint = el("div", "action-hint");
    hint.append("verify: ", elCode(`fallow trace ${file.path}#${file.unused_exports[0]}`));
    dead.appendChild(hint);
    panel.appendChild(dead);
  }

  // Complexity
  if (file.functions && file.functions.length > 0) {
    const cx = sectionEl("complexity hotspots");
    const table = el("table");
    const thead = el("thead");
    const hr = el("tr");
    for (const th of ["function", "cc", "cog", "loc", ""]) hr.appendChild(el("th", undefined, th));
    thead.appendChild(hr);
    table.appendChild(thead);
    const tbody = el("tbody");
    for (const fn of file.functions) {
      const tr = el("tr");
      const nameTd = el("td");
      nameTd.appendChild(el("span", "fn-name", fn.name));
      nameTd.appendChild(el("span", "muted", `:${fn.line}`));
      if (fn.hooks > 0 || fn.jsx_depth > 0) {
        const react = el("div", "fn-react");
        const pairs: Array<[string, number]> = [];
        if (fn.hooks > 0) pairs.push(["hooks", fn.hooks]);
        if (fn.jsx_depth > 0) pairs.push(["jsx", fn.jsx_depth]);
        if (fn.props > 0) pairs.push(["props", fn.props]);
        for (const [label, value] of pairs) {
          const pair = el("span", "pair");
          pair.appendChild(el("span", "muted", `${label} `));
          pair.appendChild(el("span", "mono", String(value)));
          react.appendChild(pair);
        }
        nameTd.appendChild(react);
      }
      tr.appendChild(nameTd);
      const ccTd = el("td", "num");
      ccTd.appendChild(sev(fn.cyclomatic >= 20 ? "sev-error" : fn.cyclomatic >= 10 ? "sev-warn" : "", String(fn.cyclomatic)));
      tr.appendChild(ccTd);
      const cogTd = el("td", "num");
      cogTd.appendChild(sev(fn.cognitive >= 25 ? "sev-error" : fn.cognitive >= 15 ? "sev-warn" : "", String(fn.cognitive)));
      tr.appendChild(cogTd);
      tr.appendChild(el("td", "num", String(fn.lines)));
      const barTd = el("td");
      barTd.appendChild(asciiBar(fn.cyclomatic, 30, 20));
      tr.appendChild(barTd);
      tbody.appendChild(tr);
    }
    table.appendChild(tbody);
    cx.appendChild(table);
    panel.appendChild(cx);
  }

  // Duplication
  if (file.clone_groups && file.clone_groups.length > 0) {
    const dup = sectionEl("duplication");
    dup.appendChild(
      el("div", "muted", `${formatCount(file.dup_lines)} duplicated lines in this file`),
    );
    for (const groupIdx of file.clone_groups.slice(0, 4)) {
      const group = state.data.clones[groupIdx];
      if (!group) continue;
      const row = el("div", "clone-row");
      const headLine = el("div", "clone-head");
      const n = el("span", "n", `${group.lines} lines`);
      headLine.appendChild(n);
      headLine.appendChild(
        document.createTextNode(` × ${group.instances.length} places`),
      );
      row.appendChild(headLine);
      const others = group.instances
        .filter((inst) => inst.file !== fileIdx)
        .map((inst) => inst.file);
      if (others.length > 0) {
        row.appendChild(linkList(state, [...new Set(others)], navigate, 5));
      }
      if (group.preview) {
        const pre = el("pre");
        pre.textContent = group.preview;
        row.appendChild(pre);
      }
      dup.appendChild(row);
    }
    if (file.clone_groups.length > 4) {
      dup.appendChild(el("div", "muted", `… ${file.clone_groups.length - 4} more clone groups`));
    }
    const hint = el("div", "action-hint");
    hint.append("explore: ", elCode(`fallow dupes --trace ${file.path}:1`));
    dup.appendChild(hint);
    panel.appendChild(dup);
  }

  // Boundary violations from this file
  const outgoing = state.data.violations.filter((v) => v.from === fileIdx);
  const incoming = state.data.violations.filter((v) => v.to === fileIdx);
  if (outgoing.length > 0 || incoming.length > 0) {
    const b = sectionEl("boundary violations");
    for (const v of outgoing.slice(0, 6)) {
      const row = el("div");
      row.appendChild(
        sev(
          "sev-error",
          `${state.data.zones[v.from_zone]?.name ?? "?"} → ${state.data.zones[v.to_zone]?.name ?? "?"} `,
        ),
      );
      const btn = el("button", undefined, basename(state.data.files[v.to].path)) as HTMLButtonElement;
      btn.type = "button";
      btn.className = "";
      btn.style.textDecoration = "underline";
      btn.addEventListener("click", () => navigate(v.to));
      row.appendChild(btn);
      row.appendChild(el("span", "muted", ` :${v.line}`));
      b.appendChild(row);
    }
    if (incoming.length > 0) {
      b.appendChild(
        el("div", "muted", `imported across a boundary by ${incoming.length} file${incoming.length === 1 ? "" : "s"}`),
      );
    }
    panel.appendChild(b);
  }

  // Cycle membership
  if (file.in_cycle) {
    const cyc = sectionEl("circular dependency");
    const cycles = state.data.cycles.filter((c) => c.includes(fileIdx));
    for (const cycle of cycles.slice(0, 2)) {
      cyc.appendChild(el("div", "sev-warn", `cycle of ${cycle.length} files`));
      cyc.appendChild(linkList(state, cycle.filter((i) => i !== fileIdx), navigate, 8));
    }
    panel.appendChild(cyc);
  }

  // Connections
  const importers = state.index.importersOf[fileIdx];
  const imports = state.index.importsOf[fileIdx];
  if (importers.length > 0) {
    const s = sectionEl(`imported by ${formatCount(importers.length)}`);
    s.appendChild(linkList(state, importers, navigate));
    panel.appendChild(s);
  }
  if (imports.length > 0) {
    const s = sectionEl(`imports ${formatCount(imports.length)}`);
    s.appendChild(linkList(state, imports, navigate));
    panel.appendChild(s);
  }
};

const elCode = (text: string): HTMLElement => {
  const code = document.createElement("code");
  code.textContent = text;
  return code;
};

/** Drill-down panel for an aggregated road: the contributing file pairs. */
const renderRoadPanel = (
  state: AppState,
  panel: HTMLElement,
  navigate: NavigateFn,
  close: () => void,
): void => {
  const road = state.selectedRoad;
  if (!road) return;
  panel.replaceChildren();
  panel.classList.add("open");

  const head = el("div", "panel-head");
  const box = el("div", "file");
  box.appendChild(el("div", "dir", "dependency road"));
  box.appendChild(el("div", "name", `${road.srcKey} ▸ ${road.dstKey}`));
  const statusLine = el("div", "status-line");
  statusLine.appendChild(
    sev("sev-info", `${formatCount(road.count)} import${road.count === 1 ? "" : "s"}`),
  );
  if (road.violations > 0) {
    statusLine.appendChild(document.createTextNode("  "));
    statusLine.appendChild(sev("sev-error", `${formatCount(road.violations)} violations`));
  }
  if (road.cycleEdges > 0) {
    statusLine.appendChild(document.createTextNode("  "));
    statusLine.appendChild(sev("sev-warn", `${formatCount(road.cycleEdges)} cycle edges`));
  }
  box.appendChild(statusLine);
  head.appendChild(box);
  const closeBtn = el("button", "close", "×") as HTMLButtonElement;
  closeBtn.type = "button";
  closeBtn.setAttribute("aria-label", "Close details");
  closeBtn.addEventListener("click", close);
  head.appendChild(closeBtn);
  panel.appendChild(head);

  const section = sectionEl(`file pairs ${formatCount(road.pairs.length)}`);
  const ul = el("ul", "link-list");
  ul.style.maxHeight = "none";
  const n = state.data.files.length;
  for (const [from, to] of road.pairs.slice(0, 80)) {
    const li = el("li");
    const btn = el("button") as HTMLButtonElement;
    btn.type = "button";
    const packed = from * n + to;
    const fromName = state.data.files[from].path;
    const toName = basename(state.data.files[to].path);
    btn.textContent = `${fromName} ▸ ${toName}`;
    if (state.index.violationEdges.has(packed)) btn.classList.add("sev-error");
    else if (state.index.cycleEdges.has(packed)) btn.classList.add("sev-warn");
    btn.addEventListener("click", () => navigate(from));
    li.appendChild(btn);
    ul.appendChild(li);
  }
  if (road.pairs.length > 80) {
    ul.appendChild(el("li", "muted", `… ${formatCount(road.pairs.length - 80)} more`));
  }
  section.appendChild(ul);
  const hint = el("div", "action-hint");
  hint.append("click a pair to open the importing file");
  section.appendChild(hint);
  panel.appendChild(section);
};
