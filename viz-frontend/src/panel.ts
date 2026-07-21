import type { AppState } from "./state";
import type { Lens, VizCloneGroup, VizFile } from "./types";
import { basename, dirname, formatCount, formatSize, reachSet } from "./data";
import { closeButton, copyButton, copyIconButton, el } from "./dom";

/** Called when the user clicks through to another file. */
export type NavigateFn = (fileIndex: number) => void;

const sectionEl = (title: string, hint?: string): HTMLElement => {
  const section = el("section");
  const heading = el("h3", undefined, title);
  // Optional "why" hangs off the header on hover instead of an always-on line
  // below the section, matching the status-label tooltips.
  if (hint) heading.dataset.tip = hint;
  section.appendChild(heading);
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
  for (const [key, value, hint] of pairs) {
    const dt = el("dt", undefined, key);
    if (hint) {
      dt.classList.add("hinted");
      dt.dataset.tip = hint;
    }
    const dd = el("dd");
    if (typeof value === "string") dd.textContent = value;
    else dd.appendChild(value);
    dl.append(dt, dd);
  }
  return dl;
};

const sev = (cls: string, text: string): HTMLElement => el("span", cls, text);

/** Which ink fills a meter: warn/error track the severity ramp; neutral is a
 *  calm fill for magnitudes that are not defects (how depended-on a file is,
 *  a query's footprint), so they never borrow the amber/red severity meaning. */
type MeterTone = "warn" | "error" | "neutral";

/** A meter specification: a value against a max, plus the ink to fill it. */
interface MeterSpec {
  value: number;
  max: number;
  tone: MeterTone;
}

const meterSpec = (value: number, max: number, tone: MeterTone): MeterSpec => ({
  value,
  max,
  tone,
});

/** An 8-slot text meter: a filled run (`█`) over a dotted track (`░`). The same
 *  motif the per-function complexity bar uses, generalized so ranked lists,
 *  blast-radius facts, and search totals can reuse it. */
const meterBar = (value: number, max: number, tone: MeterTone): HTMLElement => {
  const slots = 8;
  const filled = max > 0 ? Math.max(0, Math.min(slots, Math.round((value / max) * slots))) : 0;
  const bar = el("span", "bar");
  const fillClass =
    tone === "error" ? "fill-error" : tone === "neutral" ? "fill-neutral" : "fill-warn";
  const fill = el("span", fillClass, "█".repeat(filled));
  bar.append(fill, document.createTextNode("░".repeat(slots - filled)));
  return bar;
};

/** ASCII severity bar: the meter, red past the danger line. */
const asciiBar = (value: number, max: number, dangerAt: number): HTMLElement =>
  meterBar(value, max, value >= dangerAt ? "error" : "warn");

/** A count paired with a neutral meter of it against `max`. Shared by the
 *  facts blast-radius rows and the search totals. */
const countWithBar = (
  count: number,
  max: number,
  label: string,
  tone: MeterTone,
  labelWidthCh?: number,
): HTMLElement => {
  const wrap = el("span", "meter");
  wrap.appendChild(meterBar(count, max, tone));
  const num = el("span", "meter-num", label);
  // A fixed label width right-aligns the numbers and pins every bar to the
  // same x, so stacked rows (reaches/affects) read as an aligned mini chart.
  if (labelWidthCh !== undefined) num.style.minWidth = `${labelWidthCh}ch`;
  wrap.appendChild(num);
  return wrap;
};

const statusLabel = (file: VizFile): HTMLElement => {
  const wrap = el("span");
  switch (file.status) {
    case "unused": {
      // Why it is dead lives on hover, mirroring the entry-point label, rather
      // than on its own always-on line in the dead-code section.
      const unused = sev("sev-error", "Unused file");
      unused.dataset.tip =
        file.importer_count === 0
          ? "No file imports this one; nothing reaches it from an entry point."
          : "Unreachable from every entry point.";
      wrap.appendChild(unused);
      break;
    }
    case "hasUnusedExports":
      wrap.appendChild(
        sev(
          "sev-warn",
          `${formatCount(file.unused_export_count)} unused export${file.unused_export_count === 1 ? "" : "s"}`,
        ),
      );
      break;
    case "entryPoint": {
      // The old always-on gloss ("Where execution starts…") was redundant with
      // the label; keep the explanation on hover instead of on its own line.
      const entry = sev("sev-info", "Entry point");
      entry.dataset.tip = "Where execution starts; nothing needs to import it.";
      wrap.appendChild(entry);
      break;
    }
    default: {
      const inUse = sev("sev-ok", "In use");
      inUse.dataset.tip = "Reachable from an entry point.";
      wrap.appendChild(inUse);
    }
  }
  return wrap;
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
  const named = file.functions ?? [];
  // Anonymous arrow/callback functions are folded into a single count, not
  // listed row by row (a test file has dozens of them and they drown the
  // named functions out). fn_count is the file total; named is what's shown.
  const anonCount = Math.max(0, file.fn_count - named.length);
  if (named.length === 0 && anonCount === 0) return null;
  const cx = sectionEl("Functions");
  if (named.length > 0) {
    const table = el("table");
    const thead = el("thead");
    const hr = el("tr");
    for (const th of ["function", "cc", "cog", "loc", ""]) {
      // Numeric headers align right, over the right-aligned number cells.
      const cell = el("th", th === "cc" || th === "cog" || th === "loc" ? "num" : undefined, th);
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
    for (const fn of named) {
      const tr = el("tr");
      const nameTd = el("td");
      nameTd.appendChild(el("span", "fn-name", fn.name));
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
  }
  if (anonCount > 0) {
    cx.appendChild(
      el(
        "div",
        "muted",
        `+ ${formatCount(anonCount)} anonymous function${anonCount === 1 ? "" : "s"}`,
      ),
    );
  }
  return cx;
};

/** Clone groups this file participates in, with jump links. */
const duplicationSection = (
  state: AppState,
  file: VizFile,
  fileIdx: number,
  navigate: NavigateFn,
): HTMLElement | null => {
  if (file.clone_groups && file.clone_groups.length > 0) {
    const dup = sectionEl("Duplication");
    // Relative to the most-duplicated file, so the bar reads "how copy-pasted
    // is this file" against the worst offender in the codebase.
    const maxDupLines = state.data.files.reduce((max, other) => Math.max(max, other.dup_lines), 0);
    const density = el("div", "meter");
    density.appendChild(meterBar(file.dup_lines, maxDupLines, "warn"));
    density.appendChild(
      el("span", "muted", `${formatCount(file.dup_lines)} duplicated lines in this file`),
    );
    dup.appendChild(density);
    for (const groupIdx of file.clone_groups.slice(0, 4)) {
      const group = state.data.clones[groupIdx];
      if (!group) continue;
      const row = el("div", "clone-row");
      const headLine = el("div", "clone-head");
      const linesEl = el("span", "n", `${group.lines} lines`);
      headLine.appendChild(linesEl);
      headLine.appendChild(document.createTextNode(` × ${group.instances.length} places`));
      row.appendChild(headLine);
      const others = group.instances
        .filter((inst) => inst.file !== fileIdx)
        .map((inst) => inst.file);
      if (others.length > 0) {
        row.appendChild(fileTable(state, [...new Set(others)], navigate));
      }
      if (group.preview) {
        row.appendChild(clonePreviewEl(group));
      }
      dup.appendChild(row);
    }
    if (file.clone_groups.length > 4) {
      dup.appendChild(el("div", "muted", `… ${file.clone_groups.length - 4} more clone groups`));
    }
    dup.appendChild(commandHint("Explore", `fallow dupes --trace ${file.path}:1`));
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
  const outgoing = state.data.violations.filter((violation) => violation.from === fileIdx);
  const incoming = state.data.violations.filter((violation) => violation.to === fileIdx);
  if (outgoing.length > 0 || incoming.length > 0) {
    const section = sectionEl("Forbidden imports");
    for (const violation of outgoing.slice(0, 6)) {
      const row = el("div");
      row.appendChild(
        sev(
          "sev-error",
          `${state.data.zones[violation.from_zone]?.name ?? "?"} → ${state.data.zones[violation.to_zone]?.name ?? "?"} `,
        ),
      );
      const btn = el(
        "button",
        undefined,
        basename(state.data.files[violation.to].path),
      ) as HTMLButtonElement;
      btn.type = "button";
      btn.className = "";
      btn.style.textDecoration = "underline";
      btn.addEventListener("click", () => navigate(violation.to));
      row.appendChild(btn);
      row.appendChild(el("span", "muted", ` :${violation.line}`));
      section.appendChild(row);
    }
    if (incoming.length > 0) {
      section.appendChild(
        el(
          "div",
          "muted",
          `Imported by ${incoming.length} file${incoming.length === 1 ? "" : "s"} from outside its layer`,
        ),
      );
    }
    return section;
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
    const cyc = sectionEl("Import loop");
    const cycles = state.data.cycles.filter((cycle) => cycle.includes(fileIdx));
    for (const cycle of cycles.slice(0, 2)) {
      cyc.appendChild(el("div", "sev-warn", `Loop of ${cycle.length} files`));
      cyc.appendChild(
        fileTable(
          state,
          cycle.filter((memberIdx) => memberIdx !== fileIdx),
          navigate,
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
    const section = sectionEl(`Imported by ${formatCount(importers.length)}`);
    section.appendChild(fileTable(state, importers, navigate));
    out.push(section);
  }
  if (imports.length > 0) {
    const section = sectionEl(`Imports ${formatCount(imports.length)}`);
    section.appendChild(fileTable(state, imports, navigate));
    out.push(section);
  }
  return out;
};

/** Size, wiring, workspace, zone, and function-count facts. */
const factsSection = (state: AppState, file: VizFile, fileIdx: number): HTMLElement => {
  const facts = sectionEl("Facts");
  // Import counts live in the connection section headers below, so the
  // facts list carries only what those sections do not repeat.
  const pairs: KvPair[] = [
    ["Size", formatSize(file.size)],
    ["Exports", formatCount(file.export_count)],
  ];
  if (file.workspace !== undefined && state.data.workspaces[file.workspace]) {
    pairs.push(["Workspace", state.data.workspaces[file.workspace].name]);
  }
  if (file.zone !== undefined && state.data.zones[file.zone]) {
    pairs.push(["Zone", state.data.zones[file.zone].name]);
  }
  if (file.fn_count > 0) pairs.push(["Functions", formatCount(file.fn_count)]);
  facts.appendChild(kvEl(pairs));

  // Transitive reach: the one-look blast-radius answer. "reaches" is
  // everything this file transitively pulls in; "affects" is everything
  // that transitively depends on it (what breaks if you change it).
  if (fileIdx >= 0) {
    const reach: KvPair[] = [];
    const totalFiles = state.data.files.length;
    // Both rows share the width of the largest possible label (the whole
    // codebase), so their numbers right-align and their bars line up.
    const labelWidthCh = `${formatCount(totalFiles)} files`.length;
    const down = reachSet(state.index.importsOf, fileIdx).size;
    const up = reachSet(state.index.importersOf, fileIdx).size;
    if (down > file.import_count) {
      reach.push([
        "Reaches",
        countWithBar(down, totalFiles, `${formatCount(down)} files`, "neutral", labelWidthCh),
        "Everything this file loads, directly or through the files it imports.",
      ]);
    }
    if (up > file.importer_count) {
      reach.push([
        "Affects",
        countWithBar(up, totalFiles, `${formatCount(up)} files`, "neutral", labelWidthCh),
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
    // The "why" now rides the "Unused file" status label on hover; this section
    // is just the verify command.
    const dead = sectionEl("Dead code");
    dead.appendChild(commandHint("Verify", `fallow dead-code --trace ${file.path}`));
    return dead;
  }
  if (file.unused_exports && file.unused_exports.length > 0) {
    const dead = sectionEl("Unused exports");
    const tags = el("div", "tag-list");
    for (const name of file.unused_exports.slice(0, 20)) {
      tags.appendChild(el("span", "tag", name));
    }
    if (file.unused_exports.length > 20) {
      tags.appendChild(el("span", "muted", `… ${file.unused_exports.length - 20} more`));
    }
    dead.appendChild(tags);
    dead.appendChild(commandHint("Verify", `fallow trace ${file.path}#${file.unused_exports[0]}`));
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
  fileBox.appendChild(statusLine);
  head.appendChild(fileBox);
  // Copy path sits at the frame's bottom-right corner as an icon, mirroring
  // the close button at the top-right (positioned via CSS).
  head.appendChild(copyIconButton("copy-path", "Copy path", () => file.path));
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
  for (let match = CODE_TOKEN.exec(code); match !== null; match = CODE_TOKEN.exec(code)) {
    if (match.index > last) pre.appendChild(document.createTextNode(code.slice(last, match.index)));
    const [text, comment, str, num, ident] = match;
    let cls = "";
    if (comment !== undefined) cls = "tok-com";
    else if (str !== undefined) cls = "tok-str";
    else if (num !== undefined) cls = "tok-num";
    else if (ident !== undefined && CODE_KEYWORDS.has(ident)) cls = "tok-kw";
    if (cls) pre.appendChild(el("span", cls, text));
    else pre.appendChild(document.createTextNode(text));
    last = match.index + text.length;
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
  lines.forEach((line, lineIndex) => {
    const isDup = lineIndex >= hlStart && lineIndex < hlEnd;
    const lineEl = el("span", isDup ? "dup-line" : "ctx-line");
    highlightCode(lineEl, line);
    pre.appendChild(lineEl);
  });
  // Big blocks are truncated to the preview cap: mark the cut so a clone
  // reads as "continues below", not as if the duplication ends here.
  const hidden = group.lines - group.highlight_lines;
  if (hasRange && hidden > 0) {
    pre.appendChild(el("span", "more-line", `… ${formatCount(hidden)} more duplicated lines`));
  }
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
  row.appendChild(copyButton("cmd-copy", "Copy", () => command));
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
  /** Optional magnitude meter, drawn between the label and the value cells. */
  bar?: MeterSpec;
}

interface RankColumn {
  header: string;
  /** Column meaning, shown on the header as a `title` tooltip. */
  hint: string;
}

/** The importer-count column shared by every used-by ranked list. */
const usedByColumns: RankColumn[] = [{ header: "used by", hint: "How many files import this one." }];

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
      // codebase leans on hardest, ranked by how many import them. Reuses the
      // shared used-by row shape (fileRankRows) rather than rebuilding it.
      const ranked = files
        .map((file, index) => ({ file, index }))
        .filter(({ file }) => file.importer_count > 0)
        .toSorted((left, right) => right.file.importer_count - left.file.importer_count)
        .map(({ index }) => index);
      const rows = fileRankRows(state, ranked);
      return {
        title: "Most depended-on files",
        rows,
        empty: "No shared files",
        labelHead: "file",
        columns: usedByColumns,
      };
    }
    case "deadcode": {
      const rows: RankRow[] = [];
      const unused = files
        .map((file, index) => ({ file, index }))
        .filter(({ file }) => file.status === "unused")
        .toSorted((left, right) => right.file.size - left.file.size);
      const maxUnusedSize = unused.length > 0 ? unused[0].file.size : 0;
      for (const { file, index } of unused) {
        rows.push({
          label: basename(file.path),
          dir: dirname(file.path),
          metric: formatSize(file.size),
          cells: [{ value: formatSize(file.size), cls: "sev-error" }],
          fileIndex: index,
          bar: meterSpec(file.size, maxUnusedSize, "error"),
        });
      }
      const partial = files
        .map((file, index) => ({ file, index }))
        .filter(({ file }) => file.status !== "unused" && file.unused_export_count > 0)
        .toSorted((left, right) => right.file.unused_export_count - left.file.unused_export_count);
      const maxPartial = partial.length > 0 ? partial[0].file.unused_export_count : 0;
      for (const { file, index } of partial) {
        rows.push({
          label: basename(file.path),
          dir: dirname(file.path),
          metric: `${formatCount(file.unused_export_count)} exports`,
          cells: [{ value: `${formatCount(file.unused_export_count)} exports`, cls: "sev-warn" }],
          fileIndex: index,
          bar: meterSpec(file.unused_export_count, maxPartial, "warn"),
        });
      }
      return {
        title: "Unused files",
        rows,
        empty: "Nothing is unreachable",
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
      const groupIndices = [...state.data.clones.keys()]
        .toSorted((left, right) => state.data.clones[right].lines - state.data.clones[left].lines)
        .filter((groupIdx) => {
          // Malformed groups (no instances, or an out-of-range file
          // index) must not kill the whole ranked list.
          const group = state.data.clones[groupIdx];
          return group.instances.length > 0 && files[group.instances[0].file] !== undefined;
        });
      const maxCloneLines = groupIndices.length > 0 ? state.data.clones[groupIndices[0]].lines : 0;
      const rows = groupIndices.map((groupIdx) => {
        const group = state.data.clones[groupIdx];
        const first = group.instances[0];
        return {
          label: `${basename(files[first.file].path)} ×${group.instances.length}`,
          dir: dirname(files[first.file].path),
          metric: `${formatCount(group.lines)} lines`,
          cells: [{ value: formatCount(group.lines), cls: "sev-warn" }],
          fileIndex: first.file,
          clone: groupIdx,
          bar: meterSpec(group.lines, maxCloneLines, "warn"),
        };
      });
      const truncated = state.data.summary.clone_groups_truncated;
      const title = truncated
        ? `Duplicated blocks (+${formatCount(truncated)} not shown)`
        : "Duplicated blocks";
      return {
        title,
        rows,
        empty: "No duplicated blocks",
        labelHead: "block",
        columns: [{ header: "lines", hint: "Number of duplicated lines in the block." }],
      };
    }
    case "boundaries": {
      const rows: RankRow[] = [];
      for (const violation of state.data.violations) {
        if (files[violation.from] === undefined || files[violation.to] === undefined) continue;
        const zoneName = state.data.zones[violation.to_zone]?.name ?? "zone";
        rows.push({
          label: `${basename(files[violation.from].path)} → ${basename(files[violation.to].path)}`,
          dir: dirname(files[violation.from].path),
          metric: `→ ${zoneName}`,
          cells: [{ value: `→ ${zoneName}`, cls: "sev-error" }],
          fileIndex: violation.from,
        });
      }
      state.data.cycles.forEach((cycle) => {
        if (cycle.length === 0 || files[cycle[0]] === undefined) return;
        rows.push({
          label: `Loop of ${formatCount(cycle.length)} files`,
          dir: dirname(files[cycle[0]].path),
          metric: basename(files[cycle[0]].path),
          cells: [{ value: basename(files[cycle[0]].path), cls: "sev-warn" }],
          fileIndex: cycle[0],
        });
      });
      return {
        title: "Forbidden imports & loops",
        rows,
        empty: "No forbidden imports or loops",
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
      const risk = (file: VizFile): number =>
        file.max_cyclomatic * Math.log2(2 + file.importer_count);
      const rows = files
        .map((file, index) => ({ file, index }))
        .filter(({ file }) => file.max_cyclomatic > 0)
        .toSorted((left, right) => risk(right.file) - risk(left.file))
        .map(({ file, index }) => ({
          label: basename(file.path),
          dir: dirname(file.path),
          metric: `cc ${formatCount(file.max_cyclomatic)}, used by ${formatCount(file.importer_count)}`,
          cells: [
            {
              value: formatCount(file.max_cyclomatic),
              cls:
                file.max_cyclomatic >= 20
                  ? "sev-error"
                  : file.max_cyclomatic >= 10
                    ? "sev-warn"
                    : "",
            },
            { value: formatCount(file.importer_count), cls: "muted" },
          ],
          fileIndex: index,
          // Same 0-30 scale as the per-function bar; red past the danger line.
          bar: meterSpec(file.max_cyclomatic, 30, file.max_cyclomatic >= 20 ? "error" : "warn"),
        }));
      return {
        title: "Complexity hotspots",
        rows,
        empty: "No complex functions",
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
  onPick: (row: RankRow) => void,
): HTMLElement => {
  const { rows, labelHead, columns } = view;
  const hasBar = rows.some((row) => row.bar !== undefined);
  const table = el("table", "rank-table");
  const thead = el("thead");
  const hr = el("tr");
  hr.appendChild(el("th", "col-rank", "#"));
  hr.appendChild(el("th", "col-file", labelHead));
  if (hasBar) hr.appendChild(el("th", "col-bar"));
  for (const col of columns) {
    const th = el("th", "col-val", col.header);
    th.dataset.tip = col.hint;
    hr.appendChild(th);
  }
  thead.appendChild(hr);
  table.appendChild(thead);
  const tbody = el("tbody");
  // Every row is rendered: the panel is the only scroller, so lists never
  // hide behind an inner scrollbox or a "… N more" cutoff.
  rows.forEach((row, index) => {
    const tr = el("tr");
    tr.appendChild(el("td", "col-rank", formatCount(index + 1)));
    tr.appendChild(fileCell(row.label, row.dir ?? "", row.metric.length / 2, () => onPick(row)));
    if (hasBar) {
      const barTd = el("td", "col-bar");
      if (row.bar) barTd.appendChild(meterBar(row.bar.value, row.bar.max, row.bar.tone));
      tr.appendChild(barTd);
    }
    for (const cell of row.cells) {
      const td = el("td", "col-val");
      td.appendChild(sev(cell.cls, cell.value));
      tr.appendChild(td);
    }
    tbody.appendChild(tr);
  });
  table.appendChild(tbody);
  return table;
};

/** A numbered file list rendered as a table (rank + filename + importer count),
 *  so importers, imports, loop members, and clone copies all read as the same
 *  used-by table and carry the same "how depended-on is each" signal. Rendered
 *  in full; the panel is the only scroll. */
const fileTable = (state: AppState, indices: number[], navigate: NavigateFn): HTMLElement =>
  renderRankTable(
    state,
    { rows: fileRankRows(state, indices), labelHead: "file", columns: usedByColumns },
    (row) => navigate(row.fileIndex),
  );

/** RankRows for a set of file indices, labelled with their importer count. */
const fileRankRows = (state: AppState, indices: number[]): RankRow[] => {
  const maxImporters = indices.reduce(
    (max, index) => Math.max(max, state.data.files[index].importer_count),
    0,
  );
  return indices.map((index) => {
    const file = state.data.files[index];
    return {
      label: basename(file.path),
      dir: dirname(file.path),
      metric: `used by ${formatCount(file.importer_count)}`,
      cells: [{ value: formatCount(file.importer_count), cls: "muted" }],
      fileIndex: index,
      bar: meterSpec(file.importer_count, maxImporters, "neutral"),
    };
  });
};

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

  const section = sectionEl(
    title,
    state.lens === "hotspots"
      ? "Riskiest first: complexity weighted by how many files use it."
      : undefined,
  );
  if (rows.length === 0) {
    section.appendChild(el("div", "sev-ok", empty));
    panel.appendChild(section);
    return;
  }
  section.appendChild(
    renderRankTable(state, { rows, labelHead, columns }, (row) => {
      if (row.clone !== undefined) {
        state.selectedClone = row.clone;
        refresh();
      } else {
        navigate(row.fileIndex);
      }
    }),
  );
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
  const byImporters = (leftIdx: number, rightIdx: number): number =>
    files[rightIdx].importer_count - files[leftIdx].importer_count;
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
  const totalFiles = state.data.files.length;
  box.appendChild(el("div", "dir", `Matches for "${query}"`));
  box.appendChild(
    el("div", "name", `${formatCount(matches.length)} file${matches.length === 1 ? "" : "s"}`),
  );
  if (matches.length > 0) {
    const matchMeter = el("div", "meter");
    matchMeter.appendChild(meterBar(matches.length, totalFiles, "neutral"));
    matchMeter.appendChild(el("span", "muted", `of ${formatCount(totalFiles)} files`));
    box.appendChild(matchMeter);
  }
  if (affected.length > 0) {
    const statusLine = el("div", "status-line");
    const affectMeter = el("span", "meter");
    affectMeter.appendChild(meterBar(affected.length, totalFiles, "neutral"));
    const affects = sev("sev-info", `Affects ${formatCount(affected.length)}`);
    affects.dataset.tip = "Files that depend on these, directly or transitively.";
    affectMeter.appendChild(affects);
    statusLine.appendChild(affectMeter);
    box.appendChild(statusLine);
  }
  head.appendChild(box);
  panel.appendChild(head);

  if (matches.length === 0) {
    const empty = sectionEl("No matches");
    empty.appendChild(el("div", "muted", "No file path contains that text"));
    panel.appendChild(empty);
    return;
  }

  const section = sectionEl("Matched files", "Ranked by how many files import them.");
  section.appendChild(
    renderRankTable(
      state,
      { rows: fileRankRows(state, matches), labelHead: "file", columns: usedByColumns },
      (row) => navigate(row.fileIndex),
    ),
  );
  panel.appendChild(section);

  if (affected.length > 0) {
    const aff = sectionEl(
      `Affected files (${formatCount(affected.length)})`,
      "Everything that transitively imports a match.",
    );
    aff.appendChild(
      renderRankTable(
        state,
        { rows: fileRankRows(state, affected), labelHead: "file", columns: usedByColumns },
        (row) => navigate(row.fileIndex),
      ),
    );
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
    "Duplicated block",
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

  const copies = sectionEl(`Every copy (${formatCount(group.instances.length)})`);
  const copiesTable = el("table", "rank-table");
  const copiesHead = el("thead");
  const copiesHr = el("tr");
  copiesHr.appendChild(el("th", "col-rank", "#"));
  copiesHr.appendChild(el("th", "col-file", "file"));
  copiesHr.appendChild(el("th", "col-val", "lines"));
  copiesHead.appendChild(copiesHr);
  copiesTable.appendChild(copiesHead);
  const copiesBody = el("tbody");
  group.instances.forEach((inst, index) => {
    const path = state.data.files[inst.file].path;
    const range = `${inst.start_line}-${inst.end_line}`;
    const tr = el("tr");
    tr.appendChild(el("td", "col-rank", formatCount(index + 1)));
    tr.appendChild(
      fileCell(basename(path), dirname(path), range.length / 2, () => navigate(inst.file)),
    );
    tr.appendChild(el("td", "col-val", range));
    copiesBody.appendChild(tr);
  });
  copiesTable.appendChild(copiesBody);
  copies.appendChild(copiesTable);
  panel.appendChild(copies);

  const shared = sectionEl("The shared code");
  if (group.preview) {
    shared.appendChild(clonePreviewEl(group));
  }
  const first = group.instances[0];
  if (first) {
    shared.appendChild(
      commandHint(
        "Verify",
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
    "Imports between folders",
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

  const section = sectionEl(`Every import (${formatCount(road.pairs.length)})`);
  const importsTable = el("table", "rank-table");
  const importsHead = el("thead");
  const importsHr = el("tr");
  importsHr.appendChild(el("th", "col-rank", "#"));
  importsHr.appendChild(el("th", "col-file", "importer"));
  importsHr.appendChild(el("th", "col-val", "imports"));
  importsHead.appendChild(importsHr);
  importsTable.appendChild(importsHead);
  const importsBody = el("tbody");
  const fileCount = state.data.files.length;
  road.pairs.forEach(([from, to], index) => {
    const packed = from * fileCount + to;
    const fromPath = state.data.files[from].path;
    const toName = basename(state.data.files[to].path);
    const cls = state.index.violationEdges.has(packed)
      ? "sev-error"
      : state.index.cycleEdges.has(packed)
        ? "sev-warn"
        : "";
    const tr = el("tr");
    tr.appendChild(el("td", "col-rank", formatCount(index + 1)));
    tr.appendChild(
      fileCell(basename(fromPath), dirname(fromPath), toName.length / 2, () => navigate(from)),
    );
    const toTd = el("td", "col-val");
    toTd.appendChild(sev(cls, toName));
    tr.appendChild(toTd);
    importsBody.appendChild(tr);
  });
  importsTable.appendChild(importsBody);
  section.appendChild(importsTable);
  panel.appendChild(section);
};
