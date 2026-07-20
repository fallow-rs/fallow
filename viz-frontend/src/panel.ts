import type { AppState } from "./state";
import type { Lens, VizCloneGroup, VizFile } from "./types";
import { basename, dirname, formatCount, formatSize, reachSet } from "./data";
import { closeButton, copyButton, el } from "./dom";

/** Called when the user clicks through to another file. */
export type NavigateFn = (fileIndex: number) => void;

const sectionEl = (title: string): HTMLElement => {
  const section = el("section");
  section.appendChild(el("h3", undefined, title));
  return section;
};

/**
 * One row of the facts list. The optional third slot is the term's
 * definition: it hangs off a hover tooltip instead of a glossary line,
 * so the panel shows numbers and explains itself only when asked.
 */
type KvPair = [key: string, value: string | HTMLElement, hint?: string];

const kvEl = (pairs: KvPair[]): HTMLElement => {
  const dl = el("dl", "kv");
  for (const [k, v, hint] of pairs) {
    const dt = el("dt", undefined, k);
    if (hint) {
      dt.classList.add("hinted");
      dt.dataset.tip = hint;
    }
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
    const btn = el("button") as HTMLButtonElement;
    btn.type = "button";
    const path = state.data.files[idx].path;
    btn.appendChild(rankLabelEl(basename(path), dirname(path), 0));
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

/**
 * What the abbreviated complexity columns mean. Kept on the headers as
 * hover tooltips so the table stays a table instead of carrying a
 * glossary line under it.
 */
const COLUMN_HINTS: Record<string, string> = {
  cc: "Cyclomatic complexity: how many branches the function has.",
  cog: "Cognitive complexity: how tangled the function is to follow.",
  loc: "Lines of code in the function.",
};

/** Per-function complexity table with the React context columns. */
const complexitySection = (file: VizFile): HTMLElement | null => {
  if (file.functions && file.functions.length > 0) {
    const cx = sectionEl(
      file.fn_count > file.functions.length
        ? `complexity hotspots: top ${formatCount(file.functions.length)} of ${formatCount(file.fn_count)} functions`
        : "complexity hotspots",
    );
    const table = el("table");
    const thead = el("thead");
    const hr = el("tr");
    for (const th of ["function", "cc", "cog", "loc", ""]) {
      const cell = el("th", undefined, th);
      const hint = COLUMN_HINTS[th];
      if (hint) {
        cell.classList.add("hinted");
        cell.dataset.tip = hint;
      }
      hr.appendChild(cell);
    }
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
        row.appendChild(clonePreviewEl(group));
      }
      dup.appendChild(row);
    }
    if (file.clone_groups.length > 4) {
      dup.appendChild(el("div", "muted", `… ${file.clone_groups.length - 4} more clone groups`));
    }
    dup.appendChild(commandHint("explore", `fallow dupes --trace ${file.path}:1`));
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
  const pairs: KvPair[] = [
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
    const reach: KvPair[] = [];
    const down = reachSet(state.index.importsOf, fileIdx).size;
    const up = reachSet(state.index.importersOf, fileIdx).size;
    if (down > file.import_count) {
      reach.push([
        "reaches",
        `${formatCount(down)} files`,
        "Everything this file loads, directly or through the files it imports.",
      ]);
    }
    if (up > file.importer_count) {
      reach.push([
        "affects",
        `${formatCount(up)} files`,
        "Everything that could break if you change this file.",
      ]);
    }
    if (reach.length > 0) facts.appendChild(kvEl(reach));
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
    dead.appendChild(commandHint("verify", `fallow dead-code --trace ${file.path}`));
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
    dead.appendChild(commandHint("verify", `fallow trace ${file.path}#${file.unused_exports[0]}`));
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
    state.search,
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
  if (state.selected === null && state.search.trim() !== "") {
    // An active query owns the sidebar: the matched files and their
    // combined blast radius, not the lens list they'd otherwise see.
    renderSearchPanel(state, panel, navigate);
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

/**
 * A clone-code preview rendered line by line: the copied lines get the
 * "dup-line" highlight, the surrounding source lines are dimmed "ctx-line"
 * context, and each line keeps its syntax highlighting. A missing or zero
 * highlight range means the whole preview is the block (nothing dimmed).
 * Shared by the clone drill-down panel and the per-file duplication section.
 */
const clonePreviewEl = (group: VizCloneGroup): HTMLElement => {
  const pre = el("pre");
  const lines = group.preview.split("\n");
  const hasRange = group.highlight_lines > 0;
  const hlStart = hasRange ? group.highlight_start : 0;
  const hlEnd = hasRange ? hlStart + group.highlight_lines : lines.length;
  lines.forEach((line, i) => {
    const isDup = i >= hlStart && i < hlEnd;
    const lineEl = el("span", isDup ? "dup-line" : "ctx-line");
    highlightCode(lineEl, line);
    pre.appendChild(lineEl);
  });
  return pre;
};

/** A neat, copyable command row: a label, the command (ellipsized to fit),
 *  and a copy button. Replaces the bare "verify: <code>" action hints. */
const commandHint = (label: string, command: string): HTMLElement => {
  const row = el("div", "cmd-hint");
  row.appendChild(el("span", "cmd-label", label));
  const code = document.createElement("code");
  code.textContent = command;
  code.title = command;
  row.appendChild(code);
  row.appendChild(copyButton("cmd-copy", "copy", () => command));
  return row;
};

interface RankRow {
  label: string;
  dir?: string;
  metric: string;
  cells: { value: string; cls: string }[];
  fileIndex: number;
  /** Clone group index; rows with this open the clone panel instead. */
  clone?: number;
}

interface RankColumn {
  header: string;
  /** Column meaning, shown on the header as a `title` tooltip. */
  hint: string;
}

/** The shared shape a ranked table renders from. */
interface RankView {
  rows: RankRow[];
  labelHead: string;
  columns: RankColumn[];
}

/** A lens's ranked findings plus its section title and empty-state copy. */
interface RankLensView extends RankView {
  title: string;
  empty: string;
}

export const rankRowsFor = (state: AppState): RankLensView => rankRowsForLens(state, state.lens);

const rankRowsForLens = (state: AppState, lens: Lens): RankLensView => {
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
          cells: [{ value: formatCount(f.importer_count), cls: "muted" }],
          fileIndex: i,
        }));
      return {
        title: "most depended-on files",
        rows,
        empty: "no shared files",
        labelHead: "file",
        columns: [{ header: "used by", hint: "How many files import this one." }],
      };
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
          cells: [{ value: formatSize(f.size), cls: "sev-error" }],
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
          cells: [{ value: `${formatCount(f.unused_export_count)} exports`, cls: "sev-warn" }],
          fileIndex: i,
        });
      }
      return {
        title: "unused files",
        rows,
        empty: "nothing is unreachable",
        labelHead: "file",
        columns: [
          {
            header: "unused",
            hint: "The whole file, shown as its size on disk, or how many of its exports are never imported.",
          },
        ],
      };
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
            cells: [{ value: formatCount(group.lines), cls: "sev-warn" }],
            fileIndex: first.file,
            clone: g,
          };
        });
      const truncated = state.data.summary.clone_groups_truncated;
      const title = truncated
        ? `duplicated blocks (+${formatCount(truncated)} not shown)`
        : "duplicated blocks";
      return {
        title,
        rows,
        empty: "no duplicated blocks",
        labelHead: "block",
        columns: [{ header: "lines", hint: "Number of duplicated lines in the block." }],
      };
    }
    case "boundaries": {
      const rows: RankRow[] = [];
      for (const v of state.data.violations) {
        if (files[v.from] === undefined || files[v.to] === undefined) continue;
        const zoneName = state.data.zones[v.to_zone]?.name ?? "zone";
        rows.push({
          label: `${basename(files[v.from].path)} → ${basename(files[v.to].path)}`,
          dir: dirname(files[v.from].path),
          metric: `→ ${zoneName}`,
          cells: [{ value: `→ ${zoneName}`, cls: "sev-error" }],
          fileIndex: v.from,
        });
      }
      state.data.cycles.forEach((cycle) => {
        if (cycle.length === 0 || files[cycle[0]] === undefined) return;
        rows.push({
          label: `loop of ${formatCount(cycle.length)} files`,
          dir: dirname(files[cycle[0]].path),
          metric: basename(files[cycle[0]].path),
          cells: [{ value: basename(files[cycle[0]].path), cls: "sev-warn" }],
          fileIndex: cycle[0],
        });
      });
      return {
        title: "forbidden imports & loops",
        rows,
        empty: "no forbidden imports or loops",
        labelHead: "import",
        columns: [
          {
            header: "detail",
            hint: "The layer a forbidden import reaches into, or a file in the import loop.",
          },
        ],
      };
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
          metric: `cc ${formatCount(f.max_cyclomatic)}, used by ${formatCount(f.importer_count)}`,
          cells: [
            {
              value: formatCount(f.max_cyclomatic),
              cls: f.max_cyclomatic >= 20 ? "sev-error" : f.max_cyclomatic >= 10 ? "sev-warn" : "",
            },
            { value: formatCount(f.importer_count), cls: "muted" },
          ],
          fileIndex: i,
        }));
      return {
        title: "complexity hotspots",
        rows,
        empty: "no complex functions",
        labelHead: "file",
        columns: [
          {
            header: "cc",
            hint: "Branches in the file's hardest function; higher is harder to change.",
          },
          { header: "used by", hint: "How many files import this one." },
        ],
      };
    }
    default:
      return { title: "", rows: [], empty: "", labelHead: "", columns: [] };
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
    `# fallow map: ${state.data.root}`,
    "",
    `${formatCount(s.total_files)} files, ${formatCount(s.total_edges)} imports`,
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

/**
 * The dir-prefixed, truncating filename label shared by every ranked
 * table: a head-truncated dim directory plus the filename in its own
 * span so it can ellipsize when the row is narrow.
 */
const rankLabelEl = (label: string, dir: string, budgetHint: number): HTMLElement => {
  const labelBox = el("span", "rank-label");
  if (dir) {
    // Head-truncate the directory in JS (monospace budget), keeping
    // whole tail segments; CSS rtl tricks reorder path punctuation.
    const budget = Math.max(8, 34 - label.length - budgetHint);
    let shown = `${dir}/`;
    if (shown.length > budget) {
      const parts = dir.split("/");
      while (parts.length > 1 && `…/${parts.join("/")}/`.length > budget) parts.shift();
      shown = `…/${parts.join("/")}/`;
    }
    const dirSpan = el("span", "muted", shown);
    dirSpan.title = `${dir}/${label}`;
    labelBox.appendChild(dirSpan);
  }
  const nameSpan = el("span", "rank-name", label);
  nameSpan.title = `${dir ? `${dir}/` : ""}${label}`;
  labelBox.appendChild(nameSpan);
  return labelBox;
};

/** A clickable file cell for a rank-style table: the head-truncated dir plus
 *  the ellipsizing filename in a button. Shared by every table with a file
 *  column (lens and search rows, clone copies, road imports). */
const fileCell = (
  label: string,
  dir: string,
  budgetHint: number,
  onClick: () => void,
): HTMLElement => {
  const td = el("td", "col-file");
  const btn = el("button") as HTMLButtonElement;
  btn.type = "button";
  btn.appendChild(rankLabelEl(label, dir, budgetHint));
  btn.addEventListener("click", onClick);
  td.appendChild(btn);
  return td;
};

/**
 * The generic ranked table: a truncating label column plus one narrow,
 * right-aligned value column per definition, each carrying its meaning
 * as a header `title` tooltip. Rows stay clickable, dispatching to the
 * caller's `onPick`. Used by every lens and the search panel, so the
 * two-number complexity view and the one-number lists line up the same
 * way. Overflow past `cap` collapses to a trailing "… N more" row.
 */
const renderRankTable = (
  _state: AppState,
  view: RankView,
  cap: number,
  onPick: (row: RankRow) => void,
): HTMLElement => {
  const { rows, labelHead, columns } = view;
  const table = el("table", "rank-table");
  const thead = el("thead");
  const hr = el("tr");
  hr.appendChild(el("th", "col-file", labelHead));
  for (const col of columns) {
    const th = el("th", "col-val", col.header);
    th.dataset.tip = col.hint;
    hr.appendChild(th);
  }
  thead.appendChild(hr);
  table.appendChild(thead);
  const tbody = el("tbody");
  for (const row of rows.slice(0, cap)) {
    const tr = el("tr");
    tr.appendChild(fileCell(row.label, row.dir ?? "", row.metric.length / 2, () => onPick(row)));
    for (const cell of row.cells) {
      const td = el("td", "col-val");
      td.appendChild(sev(cell.cls, cell.value));
      tr.appendChild(td);
    }
    tbody.appendChild(tr);
  }
  if (rows.length > cap) {
    const tr = el("tr");
    const td = el(
      "td",
      "muted",
      `… ${formatCount(rows.length - cap)} more`,
    ) as HTMLTableCellElement;
    td.colSpan = 1 + columns.length;
    tr.appendChild(td);
    tbody.appendChild(tr);
  }
  table.appendChild(tbody);
  return table;
};

/** RankRows for a set of file indices, labelled with their importer count. */
const fileRankRows = (state: AppState, indices: number[]): RankRow[] =>
  indices.map((i) => {
    const f = state.data.files[i];
    return {
      label: basename(f.path),
      dir: dirname(f.path),
      metric: `used by ${formatCount(f.importer_count)}`,
      cells: [{ value: formatCount(f.importer_count), cls: "muted" }],
      fileIndex: i,
    };
  });

/** Ranked worst-first findings for the active lens (nothing selected). */
const renderLensPanel = (
  state: AppState,
  panel: HTMLElement,
  navigate: NavigateFn,
  refresh: () => void,
): void => {
  const { title, rows, empty, labelHead, columns } = rankRowsFor(state);
  panel.replaceChildren();
  panel.classList.add("open");
  panel.setAttribute("aria-label", `${state.lens} findings`);

  const section = sectionEl(title);
  if (rows.length === 0) {
    section.appendChild(el("div", "sev-ok", empty));
    panel.appendChild(section);
    return;
  }
  section.appendChild(
    renderRankTable(state, { rows, labelHead, columns }, 100, (row) => {
      if (row.clone !== undefined) {
        state.selectedClone = row.clone;
        refresh();
      } else {
        navigate(row.fileIndex);
      }
    }),
  );
  const hint = el("div", "action-hint");
  hint.append(
    state.lens === "hotspots"
      ? "Riskiest first: complexity weighted by how many files use it. Click a row to focus."
      : "Click a row to focus that file on the map.",
  );
  section.appendChild(hint);
  panel.appendChild(section);
};

/**
 * Pure model behind the search panel: the matched file indices and their
 * combined blast radius (every file that transitively imports a match),
 * each ranked most-depended-on first. Split out from the renderer so the
 * ranking is testable without a DOM, mirroring `rankRowsFor`.
 */
export const searchPanelModel = (
  state: AppState,
): { query: string; matches: number[]; affected: number[] } => {
  const files = state.data.files;
  const byImporters = (a: number, b: number): number =>
    files[b].importer_count - files[a].importer_count;
  return {
    query: state.search.trim(),
    matches: [...state.searchMatches].toSorted(byImporters),
    affected: [...state.searchReach].toSorted(byImporters),
  };
};

/**
 * Active-search view: the matched files ranked by how depended-on they
 * are, then the combined blast radius of the whole matched set (the "what
 * a PR touching these would ripple into" answer). Shown whenever a query
 * is live and no file is selected, in place of the lens list.
 */
const renderSearchPanel = (state: AppState, panel: HTMLElement, navigate: NavigateFn): void => {
  panel.replaceChildren();
  panel.classList.add("open");
  panel.setAttribute("aria-label", "search matches");

  const { query, matches, affected } = searchPanelModel(state);

  const head = el("div", "panel-head");
  const box = el("div", "file");
  box.appendChild(el("div", "dir", `matches for "${query}"`));
  box.appendChild(
    el("div", "name", `${formatCount(matches.length)} file${matches.length === 1 ? "" : "s"}`),
  );
  if (affected.length > 0) {
    const statusLine = el("div", "status-line");
    statusLine.appendChild(sev("sev-info", `affects ${formatCount(affected.length)}`));
    box.appendChild(statusLine);
    box.appendChild(
      el("div", "status-gloss", "files that depend on these, directly or transitively"),
    );
  }
  head.appendChild(box);
  panel.appendChild(head);

  if (matches.length === 0) {
    const empty = sectionEl("no matches");
    empty.appendChild(el("div", "muted", "no file path contains that text"));
    panel.appendChild(empty);
    return;
  }

  const usedByColumns = [{ header: "used by", hint: "How many files import this one." }];
  const section = sectionEl("matched files");
  section.appendChild(
    renderRankTable(
      state,
      { rows: fileRankRows(state, matches), labelHead: "file", columns: usedByColumns },
      100,
      (row) => navigate(row.fileIndex),
    ),
  );
  const matchHint = el("div", "action-hint");
  matchHint.append("Ranked by how many files import them. Click a row to focus.");
  section.appendChild(matchHint);
  panel.appendChild(section);

  if (affected.length > 0) {
    const aff = sectionEl(`affected files (${formatCount(affected.length)})`);
    aff.appendChild(
      renderRankTable(
        state,
        { rows: fileRankRows(state, affected), labelHead: "file", columns: usedByColumns },
        100,
        (row) => navigate(row.fileIndex),
      ),
    );
    const hint = el("div", "action-hint");
    hint.append("Everything that transitively imports a match. Click a row to focus.");
    aff.appendChild(hint);
    panel.appendChild(aff);
  }
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

  const copies = sectionEl(`every copy (${formatCount(group.instances.length)})`);
  const copiesTable = el("table", "rank-table");
  const copiesHead = el("thead");
  const copiesHr = el("tr");
  copiesHr.appendChild(el("th", "col-file", "file"));
  copiesHr.appendChild(el("th", "col-val", "lines"));
  copiesHead.appendChild(copiesHr);
  copiesTable.appendChild(copiesHead);
  const copiesBody = el("tbody");
  for (const inst of group.instances) {
    const path = state.data.files[inst.file].path;
    const range = `${inst.start_line}-${inst.end_line}`;
    const tr = el("tr");
    tr.appendChild(
      fileCell(basename(path), dirname(path), range.length / 2, () => navigate(inst.file)),
    );
    tr.appendChild(el("td", "col-val", range));
    copiesBody.appendChild(tr);
  }
  copiesTable.appendChild(copiesBody);
  copies.appendChild(copiesTable);
  panel.appendChild(copies);

  const shared = sectionEl("the shared code");
  if (group.preview) {
    shared.appendChild(clonePreviewEl(group));
  }
  const first = group.instances[0];
  if (first) {
    shared.appendChild(
      commandHint(
        "verify",
        `fallow dupes --trace ${state.data.files[first.file].path}:${first.start_line}`,
      ),
    );
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

  const section = sectionEl(`every import (${formatCount(road.pairs.length)})`);
  const importsTable = el("table", "rank-table");
  const importsHead = el("thead");
  const importsHr = el("tr");
  importsHr.appendChild(el("th", "col-file", "importer"));
  importsHr.appendChild(el("th", "col-val", "imports"));
  importsHead.appendChild(importsHr);
  importsTable.appendChild(importsHead);
  const importsBody = el("tbody");
  const n = state.data.files.length;
  const cap = 80;
  for (const [from, to] of road.pairs.slice(0, cap)) {
    const packed = from * n + to;
    const fromPath = state.data.files[from].path;
    const toName = basename(state.data.files[to].path);
    const cls = state.index.violationEdges.has(packed)
      ? "sev-error"
      : state.index.cycleEdges.has(packed)
        ? "sev-warn"
        : "";
    const tr = el("tr");
    tr.appendChild(
      fileCell(basename(fromPath), dirname(fromPath), toName.length / 2, () => navigate(from)),
    );
    const toTd = el("td", "col-val");
    toTd.appendChild(sev(cls, toName));
    tr.appendChild(toTd);
    importsBody.appendChild(tr);
  }
  if (road.pairs.length > cap) {
    const tr = el("tr");
    const td = el(
      "td",
      "muted",
      `… ${formatCount(road.pairs.length - cap)} more`,
    ) as HTMLTableCellElement;
    td.colSpan = 2;
    tr.appendChild(td);
    importsBody.appendChild(tr);
  }
  importsTable.appendChild(importsBody);
  section.appendChild(importsTable);
  panel.appendChild(section);
};
