import type { AppState } from "./state";
import type { Lens, VizFile } from "./types";
import { basename, dirname, formatCount, formatSize, reachSet } from "./data";
import { closeButton, copyButton, el } from "./dom";

/** Called when the user clicks through to another file. */
export type NavigateFn = (fileIndex: number) => void;

const sectionEl = (title: string): HTMLElement => {
  const section = el("section");
  section.appendChild(el("h3", undefined, title));
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
  const fill = el("span", value >= dangerAt ? "fill-error" : "fill-warn", "█".repeat(filled));
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
        sev(
          "sev-warn",
          `${formatCount(file.unused_export_count)} unused export${file.unused_export_count === 1 ? "" : "s"}`,
        ),
      );
      break;
    case "entryPoint":
      wrap.appendChild(sev("sev-info", "entry point"));
      break;
    default:
      wrap.appendChild(sev("sev-ok", "in use"));
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
  panel.setAttribute("aria-label", "file details");
  return panel;
};

/** Per-function complexity table with the React context columns. */
const complexitySection = (file: VizFile): HTMLElement | null => {
  if (file.functions && file.functions.length > 0) {
    const cx = sectionEl(
      file.fn_count > file.functions.length
        ? `complexity hotspots · top ${formatCount(file.functions.length)} of ${formatCount(file.fn_count)} functions`
        : "complexity hotspots",
    );
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
      nameTd.appendChild(el("span", "fn-name", fn.name === "<arrow>" ? "(arrow fn)" : fn.name));
      nameTd.appendChild(el("span", "muted", ` L${fn.line}`));
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
      ccTd.appendChild(
        sev(
          fn.cyclomatic >= 20 ? "sev-error" : fn.cyclomatic >= 10 ? "sev-warn" : "",
          String(fn.cyclomatic),
        ),
      );
      tr.appendChild(ccTd);
      const cogTd = el("td", "num");
      cogTd.appendChild(
        sev(
          fn.cognitive >= 25 ? "sev-error" : fn.cognitive >= 15 ? "sev-warn" : "",
          String(fn.cognitive),
        ),
      );
      tr.appendChild(cogTd);
      tr.appendChild(el("td", "num", String(fn.lines)));
      const barTd = el("td");
      barTd.appendChild(asciiBar(fn.cyclomatic, 30, 20));
      tr.appendChild(barTd);
      tbody.appendChild(tr);
    }
    table.appendChild(tbody);
    cx.appendChild(table);
    cx.appendChild(
      el("div", "muted key-line", "cc = branch count · cog = how tangled · loc = lines"),
    );
    return cx;
  }
  return null;
};

/** Clone groups this file participates in, with jump links. */
const duplicationSection = (
  state: AppState,
  file: VizFile,
  fileIdx: number,
  navigate: NavigateFn,
): HTMLElement | null => {
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
      headLine.appendChild(document.createTextNode(` × ${group.instances.length} places`));
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
    return dup;
  }
  return null;
};

/** Outgoing and incoming boundary violations. */
const boundariesSection = (
  state: AppState,
  fileIdx: number,
  navigate: NavigateFn,
): HTMLElement | null => {
  const outgoing = state.data.violations.filter((v) => v.from === fileIdx);
  const incoming = state.data.violations.filter((v) => v.to === fileIdx);
  if (outgoing.length > 0 || incoming.length > 0) {
    const b = sectionEl("forbidden imports");
    for (const v of outgoing.slice(0, 6)) {
      const row = el("div");
      row.appendChild(
        sev(
          "sev-error",
          `${state.data.zones[v.from_zone]?.name ?? "?"} → ${state.data.zones[v.to_zone]?.name ?? "?"} `,
        ),
      );
      const btn = el(
        "button",
        undefined,
        basename(state.data.files[v.to].path),
      ) as HTMLButtonElement;
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
        el(
          "div",
          "muted",
          `imported by ${incoming.length} file${incoming.length === 1 ? "" : "s"} from outside its layer`,
        ),
      );
    }
    return b;
  }
  return null;
};

/** Cycle membership with jump links. */
const cycleSection = (
  state: AppState,
  file: VizFile,
  fileIdx: number,
  navigate: NavigateFn,
): HTMLElement | null => {
  if (file.in_cycle) {
    const cyc = sectionEl("import loop");
    const cycles = state.data.cycles.filter((c) => c.includes(fileIdx));
    for (const cycle of cycles.slice(0, 2)) {
      cyc.appendChild(el("div", "sev-warn", `loop of ${cycle.length} files`));
      cyc.appendChild(
        linkList(
          state,
          cycle.filter((i) => i !== fileIdx),
          navigate,
          8,
        ),
      );
    }
    return cyc;
  }
  return null;
};

/** Importer and import link lists. */
const connectionSections = (
  state: AppState,
  fileIdx: number,
  navigate: NavigateFn,
): HTMLElement[] => {
  const out: HTMLElement[] = [];
  const importers = state.index.importersOf[fileIdx];
  const imports = state.index.importsOf[fileIdx];
  if (importers.length > 0) {
    const s = sectionEl(`imported by ${formatCount(importers.length)}`);
    s.appendChild(linkList(state, importers, navigate));
    out.push(s);
  }
  if (imports.length > 0) {
    const s = sectionEl(`imports ${formatCount(imports.length)}`);
    s.appendChild(linkList(state, imports, navigate));
    out.push(s);
  }
  return out;
};

/** Size, wiring, workspace, zone, and function-count facts. */
const factsSection = (state: AppState, file: VizFile, fileIdx: number): HTMLElement => {
  const facts = sectionEl("facts");
  // Import counts live in the connection section headers below, so the
  // facts list carries only what those sections do not repeat.
  const pairs: Array<[string, string | HTMLElement]> = [
    ["size", formatSize(file.size)],
    ["exports", formatCount(file.export_count)],
  ];
  if (file.workspace !== undefined && state.data.workspaces[file.workspace]) {
    pairs.push(["workspace", state.data.workspaces[file.workspace].name]);
  }
  if (file.zone !== undefined && state.data.zones[file.zone]) {
    pairs.push(["zone", state.data.zones[file.zone].name]);
  }
  if (file.fn_count > 0) pairs.push(["functions", formatCount(file.fn_count)]);
  facts.appendChild(kvEl(pairs));

  // Transitive reach: the one-look blast-radius answer. "reaches" is
  // everything this file transitively pulls in; "affects" is everything
  // that transitively depends on it (what breaks if you change it).
  if (fileIdx >= 0) {
    const reach: Array<[string, string | HTMLElement]> = [];
    const down = reachSet(state.index.importsOf, fileIdx).size;
    const up = reachSet(state.index.importersOf, fileIdx).size;
    if (down > file.import_count) reach.push(["reaches", `${formatCount(down)} files`]);
    if (up > file.importer_count) reach.push(["affects", `${formatCount(up)} files`]);
    if (reach.length > 0) {
      facts.appendChild(kvEl(reach));
      facts.appendChild(
        el(
          "div",
          "muted key-line",
          "reaches = everything it loads · affects = what breaks if you change it",
        ),
      );
    }
  }
  return facts;
};

/** Dead-code evidence: the unused file itself, or its unused exports. */
const deadCodeSection = (file: VizFile): HTMLElement | null => {
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
    return dead;
  }
  if (file.unused_exports && file.unused_exports.length > 0) {
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
    return dead;
  }
  return null;
};

/** Path, name, status, and the copy-path affordance. */
const fileHead = (file: VizFile, close: () => void): HTMLElement => {
  const head = el("div", "panel-head");
  const fileBox = el("div", "file");
  const dir = dirname(file.path);
  if (dir) fileBox.appendChild(el("div", "dir", `${dir}/`));
  fileBox.appendChild(el("div", "name", basename(file.path)));
  const statusLine = el("div", "status-line");
  statusLine.appendChild(statusLabel(file));
  statusLine.appendChild(copyButton("copy-path", "copy path", () => file.path));
  fileBox.appendChild(statusLine);
  const gloss =
    file.status === "clean"
      ? "reachable from an entry point"
      : file.status === "entryPoint"
        ? "where execution starts; nothing needs to import it"
        : null;
  if (gloss) fileBox.appendChild(el("div", "status-gloss", gloss));
  head.appendChild(fileBox);
  head.appendChild(closeButton(close));
  return head;
};

/**
 * Everything the panel renders from besides the static payload
 * (state.data / state.index): the selection trio and the active lens.
 * The render loop skips panel rebuilds while this key is unchanged, so
 * hover-only repaints stop reconstructing the panel DOM.
 */
export const panelRenderKey = (state: AppState): string =>
  [
    state.selected,
    state.selectedClone,
    state.selectedRoad ? `${state.selectedRoad.srcKey}>${state.selectedRoad.dstKey}` : null,
    state.lens,
  ].join("|");

export const renderPanel = (
  state: AppState,
  panel: HTMLElement,
  navigate: NavigateFn,
  close: () => void,
  refresh: () => void,
): void => {
  if (state.selected === null && state.selectedClone !== null) {
    renderClonePanel(state, panel, navigate, refresh);
    return;
  }
  if (state.selected === null && state.selectedRoad !== null) {
    renderRoadPanel(state, panel, navigate, close);
    return;
  }
  if (state.selected === null) {
    // Nothing selected: every lens shows a ranked list. Finding lenses
    // rank worst-first; overview ranks the most depended-on files, the
    // newcomer's entry point.
    renderLensPanel(state, panel, navigate, refresh);
    return;
  }

  const fileIdx = state.selected;
  const file = state.data.files[fileIdx];
  panel.replaceChildren();
  panel.classList.add("open");
  panel.setAttribute("aria-label", "file details");
  panel.appendChild(fileHead(file, close));
  panel.appendChild(factsSection(state, file, fileIdx));
  const dead = deadCodeSection(file);
  if (dead) panel.appendChild(dead);

  // Wiring first: "who imports this / what it imports" is the map's core
  // question, so answer it before the finding-specific tables.
  const sections = [
    ...connectionSections(state, fileIdx, navigate),
    complexitySection(file),
    duplicationSection(state, file, fileIdx, navigate),
    boundariesSection(state, fileIdx, navigate),
    cycleSection(state, file, fileIdx, navigate),
  ];
  for (const section of sections) {
    if (section) panel.appendChild(section);
  }
};

/** Keywords the preview highlighter tints as language syntax. */
const CODE_KEYWORDS = new Set([
  "const",
  "let",
  "var",
  "function",
  "return",
  "if",
  "else",
  "for",
  "while",
  "do",
  "switch",
  "case",
  "break",
  "continue",
  "new",
  "class",
  "extends",
  "super",
  "this",
  "import",
  "export",
  "from",
  "default",
  "async",
  "await",
  "yield",
  "typeof",
  "instanceof",
  "in",
  "of",
  "void",
  "delete",
  "try",
  "catch",
  "finally",
  "throw",
  "null",
  "true",
  "false",
  "undefined",
  "as",
  "interface",
  "type",
  "enum",
  "public",
  "private",
  "readonly",
  "static",
]);

/**
 * Minimal JS/TS syntax highlighting for the clone preview. One regex
 * splits comments, strings, numbers, and identifiers; everything else
 * stays plain. Self-contained (no dependency), good enough for a
 * read-only snippet, and preserves whitespace inside the <pre>.
 */
const CODE_TOKEN =
  /(\/\/.*|\/\*[\s\S]*?\*\/)|(`(?:\\[\s\S]|[^`\\])*`|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*')|(\b\d[\w.]*\b)|([A-Za-z_$][\w$]*)/g;

const highlightCode = (pre: HTMLElement, code: string): void => {
  let last = 0;
  for (let m = CODE_TOKEN.exec(code); m !== null; m = CODE_TOKEN.exec(code)) {
    if (m.index > last) pre.appendChild(document.createTextNode(code.slice(last, m.index)));
    const [text, comment, str, num, ident] = m;
    let cls = "";
    if (comment !== undefined) cls = "tok-com";
    else if (str !== undefined) cls = "tok-str";
    else if (num !== undefined) cls = "tok-num";
    else if (ident !== undefined && CODE_KEYWORDS.has(ident)) cls = "tok-kw";
    if (cls) pre.appendChild(el("span", cls, text));
    else pre.appendChild(document.createTextNode(text));
    last = m.index + text.length;
  }
  if (last < code.length) pre.appendChild(document.createTextNode(code.slice(last)));
};

const elCode = (text: string): HTMLElement => {
  const code = document.createElement("code");
  code.textContent = text;
  return code;
};

interface RankRow {
  label: string;
  dir?: string;
  metric: string;
  metricCls: string;
  fileIndex: number;
  /** Clone group index; rows with this open the clone panel instead. */
  clone?: number;
}

export const rankRowsFor = (state: AppState): { title: string; rows: RankRow[]; empty: string } =>
  rankRowsForLens(state, state.lens);

const rankRowsForLens = (
  state: AppState,
  lens: Lens,
): { title: string; rows: RankRow[]; empty: string } => {
  const files = state.data.files;
  switch (lens) {
    case "overview": {
      // The newcomer's "what should I read first": files the rest of the
      // codebase leans on hardest, ranked by how many import them.
      const rows = files
        .map((f, i) => ({ f, i }))
        .filter(({ f }) => f.importer_count > 0)
        .toSorted((a, b) => b.f.importer_count - a.f.importer_count)
        .map(({ f, i }) => ({
          label: basename(f.path),
          dir: dirname(f.path),
          metric: `used by ${formatCount(f.importer_count)}`,
          metricCls: "muted",
          fileIndex: i,
        }));
      return { title: "most depended-on files", rows, empty: "no shared files" };
    }
    case "deadcode": {
      const rows: RankRow[] = [];
      const unused = files
        .map((f, i) => ({ f, i }))
        .filter(({ f }) => f.status === "unused")
        .toSorted((a, b) => b.f.size - a.f.size);
      for (const { f, i } of unused) {
        rows.push({
          label: basename(f.path),
          dir: dirname(f.path),
          metric: formatSize(f.size),
          metricCls: "sev-error",
          fileIndex: i,
        });
      }
      const partial = files
        .map((f, i) => ({ f, i }))
        .filter(({ f }) => f.status !== "unused" && f.unused_export_count > 0)
        .toSorted((a, b) => b.f.unused_export_count - a.f.unused_export_count);
      for (const { f, i } of partial) {
        rows.push({
          label: basename(f.path),
          dir: dirname(f.path),
          metric: `${formatCount(f.unused_export_count)} exports`,
          metricCls: "sev-warn",
          fileIndex: i,
        });
      }
      return { title: "unused files", rows, empty: "nothing is unreachable" };
    }
    case "dupes": {
      const rows = [...state.data.clones.keys()]
        .toSorted((a, b) => state.data.clones[b].lines - state.data.clones[a].lines)
        .filter((g) => {
          // Malformed groups (no instances, or an out-of-range file
          // index) must not kill the whole ranked list.
          const group = state.data.clones[g];
          return group.instances.length > 0 && files[group.instances[0].file] !== undefined;
        })
        .map((g) => {
          const group = state.data.clones[g];
          const first = group.instances[0];
          return {
            label: `${basename(files[first.file].path)} ×${group.instances.length}`,
            dir: dirname(files[first.file].path),
            metric: `${formatCount(group.lines)} lines`,
            metricCls: "sev-warn",
            fileIndex: first.file,
            clone: g,
          };
        });
      const truncated = state.data.summary.clone_groups_truncated;
      const title = truncated
        ? `duplicated blocks · +${formatCount(truncated)} not shown`
        : "duplicated blocks";
      return { title, rows, empty: "no duplicated blocks" };
    }
    case "boundaries": {
      const rows: RankRow[] = [];
      for (const v of state.data.violations) {
        if (files[v.from] === undefined || files[v.to] === undefined) continue;
        rows.push({
          label: `${basename(files[v.from].path)} → ${basename(files[v.to].path)}`,
          dir: dirname(files[v.from].path),
          metric: `→ ${state.data.zones[v.to_zone]?.name ?? "zone"}`,
          metricCls: "sev-error",
          fileIndex: v.from,
        });
      }
      state.data.cycles.forEach((cycle) => {
        if (cycle.length === 0 || files[cycle[0]] === undefined) return;
        rows.push({
          label: `loop · ${formatCount(cycle.length)} files`,
          dir: dirname(files[cycle[0]].path),
          metric: basename(files[cycle[0]].path),
          metricCls: "sev-warn",
          fileIndex: cycle[0],
        });
      });
      return { title: "forbidden imports & loops", rows, empty: "no forbidden imports or loops" };
    }
    case "hotspots": {
      // Risk = hard to change AND widely depended on, not hardness alone;
      // that is the "what do we refactor first" ordering a lead wants.
      const risk = (f: VizFile): number => f.max_cyclomatic * Math.log2(2 + f.importer_count);
      const rows = files
        .map((f, i) => ({ f, i }))
        .filter(({ f }) => f.max_cyclomatic > 0)
        .toSorted((a, b) => risk(b.f) - risk(a.f))
        .map(({ f, i }) => ({
          label: basename(f.path),
          dir: dirname(f.path),
          metric: `cc ${formatCount(f.max_cyclomatic)} · used by ${formatCount(f.importer_count)}`,
          metricCls:
            f.max_cyclomatic >= 20 ? "sev-error" : f.max_cyclomatic >= 10 ? "sev-warn" : "muted",
          fileIndex: i,
        }));
      return { title: "complexity hotspots", rows, empty: "no complex functions" };
    }
    default:
      return { title: "", rows: [], empty: "" };
  }
};

/** Per-lens CLI that reproduces its findings, mirroring the summary chip. */
const DIGEST_COMMANDS: Record<Lens, string> = {
  overview: "fallow",
  deadcode: "fallow dead-code",
  dupes: "fallow dupes",
  boundaries: "fallow dead-code --circular-deps --boundary-violations",
  hotspots: "fallow health",
};

/**
 * One markdown block covering the whole map: headline totals, then the
 * top rows of every lens with the command that verifies each. The single
 * paste an agent (or a quarterly report) wants, assembled from data
 * already in memory.
 */
export const buildMapDigest = (state: AppState): string => {
  const s = state.data.summary;
  const lines: string[] = [
    `# fallow map · ${state.data.root}`,
    "",
    `${formatCount(s.total_files)} files · ${formatCount(s.total_edges)} imports`,
    `- unused: ${formatCount(s.unused_files)} files, ${formatCount(s.unused_exports)} exports`,
    `- duplication: ${formatCount(s.clone_groups)} groups${
      s.clone_groups_truncated ? ` (+${formatCount(s.clone_groups_truncated)} not shown)` : ""
    }, ${formatCount(s.duplicated_lines)} lines`,
    `- boundaries: ${formatCount(s.circular_deps)} loops, ${formatCount(s.boundary_violations)} forbidden imports`,
    `- complexity: ${formatCount(s.hotspot_files)} files flagged as hardest to change`,
  ];
  const lenses: Lens[] = ["overview", "deadcode", "dupes", "boundaries", "hotspots"];
  for (const lens of lenses) {
    const { title, rows } = rankRowsForLens(state, lens);
    if (rows.length === 0) continue;
    lines.push("", `## ${title}`, `\`$ ${DIGEST_COMMANDS[lens]}\``, "");
    for (const row of rows.slice(0, 15)) {
      lines.push(`- ${row.dir ? `${row.dir}/` : ""}${row.label} (${row.metric})`);
    }
    if (rows.length > 15) lines.push(`- … ${formatCount(rows.length - 15)} more`);
  }
  return lines.join("\n");
};

/** Ranked worst-first findings for the active lens (nothing selected). */
const renderLensPanel = (
  state: AppState,
  panel: HTMLElement,
  navigate: NavigateFn,
  refresh: () => void,
): void => {
  const { title, rows, empty } = rankRowsFor(state);
  panel.replaceChildren();
  panel.classList.add("open");
  panel.setAttribute("aria-label", `${state.lens} findings`);

  const section = sectionEl(title);
  if (rows.length === 0) {
    section.appendChild(el("div", "sev-ok", empty));
    panel.appendChild(section);
    return;
  }
  // Leave with the list, not just a feeling: findings as markdown.
  section.appendChild(
    copyButton("copy-path", "copy this list", () =>
      [
        `# fallow · ${title} · ${state.data.root}`,
        ...rows.map((r) => `- ${r.dir ? `${r.dir}/` : ""}${r.label} (${r.metric})`),
      ].join("\n"),
    ),
  );
  const cap = 100;
  const ul = el("ul", "link-list rank-list");
  for (const row of rows.slice(0, cap)) {
    const li = el("li");
    const btn = el("button") as HTMLButtonElement;
    btn.type = "button";
    const labelBox = el("span", "rank-label");
    if (row.dir) {
      // Head-truncate the directory in JS (monospace budget), keeping
      // whole tail segments; CSS rtl tricks reorder path punctuation.
      const budget = Math.max(8, 34 - row.label.length - row.metric.length / 2);
      let dir = `${row.dir}/`;
      if (dir.length > budget) {
        const parts = row.dir.split("/");
        while (parts.length > 1 && `…/${parts.join("/")}/`.length > budget) parts.shift();
        dir = `…/${parts.join("/")}/`;
      }
      const dirSpan = el("span", "muted", dir);
      dirSpan.title = `${row.dir}/${row.label}`;
      labelBox.appendChild(dirSpan);
    }
    labelBox.appendChild(document.createTextNode(row.label));
    btn.appendChild(labelBox);
    btn.appendChild(el("span", `rank-metric ${row.metricCls}`, row.metric));
    btn.addEventListener("click", () => {
      if (row.clone !== undefined) {
        state.selectedClone = row.clone;
        refresh();
      } else {
        navigate(row.fileIndex);
      }
    });
    li.appendChild(btn);
    ul.appendChild(li);
  }
  if (rows.length > cap) {
    ul.appendChild(el("li", "muted", `… ${formatCount(rows.length - cap)} more`));
  }
  section.appendChild(ul);
  const hint = el("div", "action-hint");
  hint.append(
    state.lens === "hotspots"
      ? "riskiest first = complexity weighted by how many files use it · click a row to focus"
      : "click a row to focus that file on the map",
  );
  section.appendChild(hint);
  panel.appendChild(section);
};

/** Clone-group drill-down: the preview plus every copy as a jump link. */
const renderClonePanel = (
  state: AppState,
  panel: HTMLElement,
  navigate: NavigateFn,
  refresh: () => void,
): void => {
  const groupIdx = state.selectedClone;
  const group = groupIdx !== null ? state.data.clones[groupIdx] : undefined;
  if (groupIdx === null || !group) return;
  const box = panelShell(
    panel,
    "duplicated block",
    `${formatCount(group.lines)} lines × ${formatCount(group.instances.length)} places`,
    () => {
      state.selectedClone = null;
      refresh();
    },
  );
  panel.setAttribute("aria-label", "duplicated block");
  const statusLine = el("div", "status-line");
  statusLine.appendChild(sev("sev-warn", `${formatCount(group.tokens)} tokens`));
  box.appendChild(statusLine);

  const copies = sectionEl(`every copy · ${formatCount(group.instances.length)}`);
  const ul = el("ul", "link-list");
  for (const inst of group.instances) {
    const li = el("li");
    const btn = el("button") as HTMLButtonElement;
    btn.type = "button";
    btn.textContent = `${state.data.files[inst.file].path}:${inst.start_line}-${inst.end_line}`;
    btn.addEventListener("click", () => navigate(inst.file));
    li.appendChild(btn);
    ul.appendChild(li);
  }
  copies.appendChild(ul);
  panel.appendChild(copies);

  const shared = sectionEl("the shared code");
  if (group.preview) {
    const pre = el("pre");
    highlightCode(pre, group.preview);
    shared.appendChild(pre);
  }
  const first = group.instances[0];
  if (first) {
    const hint = el("div", "action-hint");
    hint.append(
      "verify: ",
      elCode(`fallow dupes --trace ${state.data.files[first.file].path}:${first.start_line}`),
    );
    shared.appendChild(hint);
  }
  if (shared.childNodes.length > 1) panel.appendChild(shared);
};

/**
 * Open the panel with the shared head shell (eyebrow + title) used by
 * the drill-down panels; returns the box for extra status lines.
 */
const panelShell = (
  panel: HTMLElement,
  eyebrow: string,
  title: string,
  onClose: () => void,
): HTMLElement => {
  panel.replaceChildren();
  panel.classList.add("open");
  const head = el("div", "panel-head");
  const box = el("div", "file");
  box.appendChild(el("div", "dir", eyebrow));
  box.appendChild(el("div", "name", title));
  head.appendChild(box);
  head.appendChild(closeButton(onClose));
  panel.appendChild(head);
  return box;
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
  const box = panelShell(
    panel,
    "imports between folders",
    `${road.srcKey} → ${road.dstKey}`,
    close,
  );
  panel.setAttribute("aria-label", "imports between folders");
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

  const section = sectionEl(`every import · ${formatCount(road.pairs.length)}`);
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
    btn.textContent = `${fromName} → ${toName}`;
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
