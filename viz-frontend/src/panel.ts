import type { AppState } from "./state";
import type { Lens, SecondaryAnalysis, VizCloneGroup, VizFile } from "./types";
import {
  analysisAvailability,
  basename,
  dirname,
  findingsForAnalysis,
  findingsForFile,
  formatCount,
  formatSize,
  healthRiskForFile,
  healthHasFindingForFile,
  reachSet,
  securityBlindSpots,
  securityBlindSpotsForFile,
  securityCandidatesForFile,
  securityRuntimeAvailability,
} from "./data";
import type {
  AnalysisAvailabilityView,
  AnalysisId,
  FindingActionView,
  GenericFindingView,
  SecurityCandidateView,
  SecurityBlindSpotView,
} from "./data";
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

interface FileSignalModel {
  id: AnalysisId | "overview";
  label: string;
  count: number;
  state: AnalysisAvailabilityView["state"];
  active: boolean;
}

export interface FilePanelModel {
  active: AnalysisId | "overview";
  signals: FileSignalModel[];
}

const SIGNAL_LABELS: Record<AnalysisId | "overview", string> = {
  overview: "Overview",
  unused: "Unused",
  duplication: "Duplication",
  architecture: "Architecture",
  health: "Health",
  security: "Security",
  dependencies: "Dependencies",
  frameworks: "Frameworks",
  styling: "Styling",
  flags: "Feature flags",
};

const SIGNAL_STATE_SUFFIX: Record<
  Exclude<AnalysisAvailabilityView["state"], "complete">,
  string
> = {
  disabled: " disabled",
  notApplicable: " not applicable",
  unavailable: " unavailable",
};

const analysisIdForLens = (lens: Lens): AnalysisId | "overview" => {
  switch (lens) {
    case "unused":
      return "unused";
    case "duplication":
      return "duplication";
    case "architecture":
      return "architecture";
    case "health":
      return "health";
    case "security":
      return "security";
    default:
      return "overview";
  }
};

const analysisIdForSecondary = (analysis: SecondaryAnalysis): AnalysisId =>
  analysis === "feature_flags" ? "flags" : analysis;

const activeAnalysisId = (state: AppState): AnalysisId | "overview" =>
  state.activeAnalysis === null
    ? analysisIdForLens(state.lens)
    : analysisIdForSecondary(state.activeAnalysis);

const genericCountForFile = (state: AppState, id: AnalysisId, fileIdx: number): number => {
  if (id === "security") return securityCandidatesForFile(state.data, fileIdx).length;
  if (id === "unused") {
    const file = state.data.files[fileIdx];
    return file.status === "unused" ? 1 : file.unused_export_count;
  }
  if (id === "duplication") return state.data.files[fileIdx].clone_groups?.length ?? 0;
  if (id === "architecture") return findingsForFile(state.data, "architecture", fileIdx).length;
  return findingsForFile(state.data, id, fileIdx).length;
};

/** Pure file-panel ordering model used by the renderer and tests. */
export const filePanelModel = (state: AppState, fileIdx: number): FilePanelModel => {
  const active = activeAnalysisId(state);
  const ids: Array<AnalysisId | "overview"> = [
    "overview",
    "unused",
    "duplication",
    "architecture",
    "health",
    "security",
    "dependencies",
    "frameworks",
    "styling",
    "flags",
  ];
  return {
    active,
    signals: ids.map((id) => {
      const availability =
        id === "overview"
          ? ({ state: "complete", count: 1, unit: "file" } satisfies AnalysisAvailabilityView)
          : analysisAvailability(state.data, id);
      return {
        id,
        label: SIGNAL_LABELS[id],
        count: id === "overview" ? 1 : genericCountForFile(state, id, fileIdx),
        state: availability.state,
        active: id === active,
      };
    }),
  };
};

const sectionId = (id: AnalysisId | "overview", supporting: boolean): string =>
  `panel-${supporting ? "supporting" : "active"}-${id}`;

const signalNavigator = (model: FilePanelModel): HTMLElement => {
  const nav = el("nav", "signal-nav");
  nav.setAttribute("aria-label", "File signals");
  for (const signal of model.signals) {
    if (!signal.active && signal.count === 0 && signal.state === "complete") continue;
    const button = el("button") as HTMLButtonElement;
    button.type = "button";
    button.className = signal.active ? "active" : "";
    button.setAttribute("aria-current", signal.active ? "true" : "false");
    const suffix =
      signal.id === "overview"
        ? ""
        : signal.state === "complete"
          ? ` ${formatCount(signal.count)}`
          : SIGNAL_STATE_SUFFIX[signal.state];
    button.textContent = `${signal.label}${suffix}`;
    button.addEventListener("click", () => {
      const target = document.getElementById(sectionId(signal.id, !signal.active));
      target?.scrollIntoView({ block: "nearest" });
    });
    nav.appendChild(button);
  }
  return nav;
};

const availabilityMessage = (
  id: AnalysisId,
  availability: AnalysisAvailabilityView,
): HTMLElement => {
  if (availability.state === "complete") {
    return el("div", "sev-ok", `No ${availability.unit} for this file`);
  }
  const labels: Record<Exclude<AnalysisAvailabilityView["state"], "complete">, string> = {
    disabled: "Disabled",
    notApplicable: "Not applicable",
    unavailable: "Unavailable",
  };
  const message = el("div", "availability-state");
  message.appendChild(sev("sev-info", labels[availability.state]));
  message.appendChild(
    document.createTextNode(
      availability.reason ? `: ${availability.reason}` : ` for ${SIGNAL_LABELS[id]}`,
    ),
  );
  return message;
};

const findingActions = (actions: FindingActionView[]): HTMLElement | null => {
  if (actions.length === 0) return null;
  const wrap = el("div", "finding-actions");
  for (const action of actions) {
    if (action.command) wrap.appendChild(commandHint(action.label, action.command));
    else if (action.description) {
      wrap.appendChild(el("div", "muted", `${action.label}: ${action.description}`));
    }
  }
  if (wrap.childNodes.length === 0) return null;
  return wrap;
};

const securityCandidateEl = (
  candidate: SecurityCandidateView,
  showOnMap: (() => void) | null,
): HTMLElement => {
  const article = el("article", "security-candidate");
  const heading = el("h4", undefined, candidate.title);
  article.appendChild(heading);
  const badges = el("div", "status-line");
  badges.appendChild(
    sev(
      ["critical", "high", "error"].includes(candidate.severity.toLowerCase())
        ? "sev-error"
        : "sev-warn",
      candidate.severity,
    ),
  );
  if (candidate.confidence)
    badges.appendChild(sev("sev-info", `${candidate.confidence} confidence`));
  article.appendChild(badges);
  const facts: KvPair[] = [];
  if (candidate.category) facts.push(["Category", candidate.category]);
  if (candidate.cwe) facts.push(["CWE", candidate.cwe]);
  if (candidate.line !== null) {
    facts.push([
      "Location",
      `${candidate.path}:${candidate.line}${candidate.column === null ? "" : `:${candidate.column}`}`,
    ]);
  }
  if (candidate.source) facts.push(["Source", candidate.source]);
  if (candidate.sink) facts.push(["Sink", candidate.sink]);
  if (candidate.urlShape) facts.push(["URL shape", candidate.urlShape]);
  if (candidate.networkDestination) {
    facts.push(["Network destination", candidate.networkDestination]);
  }
  if (candidate.boundary) facts.push(["Trust boundary", candidate.boundary]);
  if (candidate.reachability) facts.push(["Reachability", candidate.reachability]);
  if (candidate.blastRadius !== null) {
    facts.push(["Blast radius", `${formatCount(candidate.blastRadius)} files`]);
  }
  if (candidate.deadCode !== null) facts.push(["Dead code", candidate.deadCode ? "Yes" : "No"]);
  if (candidate.runtime) facts.push(["Runtime evidence", candidate.runtime]);
  if (facts.length > 0) article.appendChild(kvEl(facts));
  if (candidate.evidence) article.appendChild(el("p", "finding-evidence", candidate.evidence));
  if (candidate.trace.length > 0) {
    const trace = el("ol", "trace-list");
    for (const step of candidate.trace) trace.appendChild(el("li", undefined, step));
    article.appendChild(trace);
  }
  if (candidate.taintFlow) {
    article.appendChild(el("p", "finding-evidence", `Taint flow: ${candidate.taintFlow}`));
  }
  if (candidate.observedControls.length > 0) {
    const controls = el("div", "observed-controls");
    controls.appendChild(el("strong", undefined, "Observed controls"));
    const list = el("ul");
    for (const control of candidate.observedControls)
      list.appendChild(el("li", undefined, control));
    controls.appendChild(list);
    article.appendChild(controls);
  }
  if (candidate.verificationPrompt) {
    article.appendChild(
      el("p", "finding-evidence", `Verify controls: ${candidate.verificationPrompt}`),
    );
  }
  const actions = findingActions(candidate.actions);
  if (actions) article.appendChild(actions);
  const localActions = el("div", "finding-actions");
  localActions.appendChild(copyButton("finding-copy", "Copy finding ID", () => candidate.id));
  if (candidate.evidence) {
    localActions.appendChild(
      copyButton("finding-copy", "Copy evidence", () => candidate.evidence ?? ""),
    );
  }
  if (showOnMap) {
    const show = el("button", undefined, "Show on map") as HTMLButtonElement;
    show.type = "button";
    show.addEventListener("click", showOnMap);
    localActions.appendChild(show);
  }
  article.appendChild(localActions);
  return article;
};

const genericFindingEl = (finding: GenericFindingView): HTMLElement => {
  const article = el("article", "analysis-finding");
  const heading = el("h4", undefined, finding.title);
  if (finding.severity) {
    heading.appendChild(
      sev(
        ["critical", "high", "error"].includes(finding.severity.toLowerCase())
          ? "sev-error"
          : "sev-warn",
        ` ${finding.severity}`,
      ),
    );
  }
  article.appendChild(heading);
  if (finding.path) {
    article.appendChild(
      el("div", "muted", `${finding.path}${finding.line === null ? "" : `:${finding.line}`}`),
    );
  }
  if (finding.detail) article.appendChild(el("p", "finding-evidence", finding.detail));
  if (finding.metrics.length > 0) article.appendChild(kvEl(finding.metrics));
  const actions = findingActions(finding.actions);
  if (actions) article.appendChild(actions);
  return article;
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

const genericAnalysisSection = (
  state: AppState,
  id: Exclude<AnalysisId, "unused" | "duplication" | "security">,
  fileIdx: number,
  excludedKinds: ReadonlySet<string> = new Set(),
): HTMLElement => {
  const availability = analysisAvailability(state.data, id);
  const section = sectionEl(SIGNAL_LABELS[id]);
  const findings = findingsForFile(state.data, id, fileIdx).filter(
    (finding) => !excludedKinds.has(finding.kind),
  );
  if (availability.state !== "complete" || findings.length === 0) {
    section.appendChild(availabilityMessage(id, availability));
    return section;
  }
  for (const finding of findings) section.appendChild(genericFindingEl(finding));
  return section;
};

const blindSpotEl = (blindSpot: SecurityBlindSpotView): HTMLElement => {
  const location = blindSpot.path
    ? `${blindSpot.path}${blindSpot.line === null ? "" : `:${blindSpot.line}`}`
    : null;
  return el(
    "p",
    "availability-state",
    `${blindSpot.kind.endsWith("-sample") ? "Sample: " : ""}${blindSpot.kind} (${formatCount(blindSpot.count)})${location ? ` at ${location}` : ""}${blindSpot.reason ? `: ${blindSpot.reason}` : ""}`,
  );
};

const securitySection = (state: AppState, fileIdx: number, navigate: NavigateFn): HTMLElement => {
  const availability = analysisAvailability(state.data, "security");
  const section = sectionEl("Static Security candidates");
  const candidates = securityCandidatesForFile(state.data, fileIdx);
  if (availability.state !== "complete" || candidates.length === 0) {
    section.appendChild(availabilityMessage("security", availability));
  } else {
    const note = el(
      "p",
      "muted",
      "Candidates need review. Static evidence does not by itself confirm a vulnerability.",
    );
    section.appendChild(note);
    for (const candidate of candidates) {
      section.appendChild(
        securityCandidateEl(
          candidate,
          candidate.fileIndex === null ? null : () => navigate(candidate.fileIndex ?? fileIdx),
        ),
      );
    }
  }
  const blindSpots = securityBlindSpotsForFile(state.data, fileIdx);
  if (blindSpots.length > 0) {
    const details = el("details", "supporting-signals") as HTMLDetailsElement;
    details.appendChild(el("summary", undefined, `Blind spots ${formatCount(blindSpots.length)}`));
    for (const blindSpot of blindSpots) {
      details.appendChild(blindSpotEl(blindSpot));
    }
    section.appendChild(details);
  }
  const runtime = securityRuntimeAvailability(state.data);
  const runtimeState = el("div", "availability-state");
  runtimeState.appendChild(el("strong", undefined, "Runtime Security: "));
  runtimeState.appendChild(
    document.createTextNode(
      runtime.state === "complete"
        ? `${formatCount(runtime.count)} ${runtime.unit}`
        : `${runtime.state}${runtime.reason ? `, ${runtime.reason}` : ""}`,
    ),
  );
  section.appendChild(runtimeState);
  return section;
};

const securityCoverageSection = (state: AppState): HTMLElement => {
  const section = sectionEl("Security coverage");
  const runtime = securityRuntimeAvailability(state.data);
  section.appendChild(
    el(
      "div",
      "availability-state",
      runtime.state === "complete"
        ? `Runtime Security: ${formatCount(runtime.count)} ${runtime.unit}`
        : `Runtime Security: ${runtime.state}${runtime.reason ? `, ${runtime.reason}` : ""}`,
    ),
  );
  const blindSpots = securityBlindSpots(state.data);
  if (state.data.security.blind_spot_count === 0) {
    section.appendChild(el("div", "sev-ok", "No static-analysis blind spots reported"));
    return section;
  }
  const details = el("details", "supporting-signals") as HTMLDetailsElement;
  const truncated = state.data.security.blind_spots_truncated ?? 0;
  details.appendChild(
    el(
      "summary",
      undefined,
      `${formatCount(state.data.security.blind_spot_count)} blind spots${truncated > 0 ? `, ${formatCount(truncated)} samples not shown` : ""}`,
    ),
  );
  for (const blindSpot of blindSpots) {
    details.appendChild(blindSpotEl(blindSpot));
  }
  section.appendChild(details);
  return section;
};

const healthSummarySection = (state: AppState): HTMLElement | null => {
  const pairs: KvPair[] = [];
  if (state.data.health.score !== undefined) {
    pairs.push(["Score", state.data.health.score.toFixed(1)]);
  }
  if (state.data.health.grade) pairs.push(["Grade", state.data.health.grade]);
  if (state.data.health.average_maintainability !== undefined) {
    pairs.push(["Maintainability", state.data.health.average_maintainability.toFixed(1)]);
  }
  if (pairs.length === 0) return null;
  const section = sectionEl("Project Health");
  section.appendChild(kvEl(pairs));
  const capabilities = el("details", "supporting-signals") as HTMLDetailsElement;
  capabilities.appendChild(el("summary", undefined, "Health signal coverage"));
  for (const [label, availability] of Object.entries(state.data.health.capabilities)) {
    capabilities.appendChild(
      el(
        "p",
        "availability-state",
        `${label}: ${availability.state === "complete" ? `${formatCount(availability.count)} ${availability.unit}` : `${availability.state}${availability.reason ? `, ${availability.reason}` : ""}`}`,
      ),
    );
  }
  section.appendChild(capabilities);
  return section;
};

const frameworkSummarySection = (state: AppState): HTMLElement | null => {
  const detectorAvailability = state.data.frameworks.detector_availability;
  const section = sectionEl("Framework detector coverage");
  if (state.data.frameworks.detected_frameworks.length > 0) {
    section.appendChild(el("p", undefined, state.data.frameworks.detected_frameworks.join(", ")));
  }
  for (const detector of state.data.frameworks.detectors) {
    section.appendChild(
      el(
        "p",
        "availability-state",
        `${detector.id}: ${detector.status}${detector.reason ? `, ${detector.reason}` : ""}`,
      ),
    );
  }
  if (state.data.frameworks.detectors.length === 0) {
    section.appendChild(
      el(
        "p",
        "availability-state",
        `${detectorAvailability.state}${detectorAvailability.reason ? `, ${detectorAvailability.reason}` : ""}`,
      ),
    );
  }
  return section;
};

const stylingSummarySection = (state: AppState): HTMLElement | null => {
  const pairs: KvPair[] = [];
  if (state.data.styling.score !== undefined)
    pairs.push(["Score", state.data.styling.score.toFixed(1)]);
  if (state.data.styling.grade) pairs.push(["Grade", state.data.styling.grade]);
  if (state.data.styling.confidence) pairs.push(["Confidence", state.data.styling.confidence]);
  const summary = state.data.styling.summary;
  if (typeof summary === "object" && summary !== null) {
    for (const [label, key] of [
      ["Stylesheets", "files_analyzed"],
      ["Rules", "total_rules"],
      ["Declarations", "total_declarations"],
      ["Unique colors", "unique_colors"],
    ] as const) {
      const value = Reflect.get(summary, key);
      if (typeof value === "number") pairs.push([label, formatCount(value)]);
    }
  }
  if (pairs.length === 0) return null;
  const section = sectionEl("Styling Health");
  section.appendChild(kvEl(pairs));
  return section;
};

const analysisContent = (
  state: AppState,
  id: AnalysisId | "overview",
  file: VizFile,
  fileIdx: number,
  navigate: NavigateFn,
): HTMLElement[] => {
  if (id === "overview") {
    return [factsSection(state, file, fileIdx), ...connectionSections(state, fileIdx, navigate)];
  }
  if (id === "unused") {
    const finding = deadCodeSection(file);
    if (finding) return [finding];
    const section = sectionEl("Unused");
    section.appendChild(availabilityMessage(id, analysisAvailability(state.data, id)));
    return [section];
  }
  if (id === "duplication") {
    const finding = duplicationSection(state, file, fileIdx, navigate);
    if (finding) return [finding];
    const section = sectionEl("Duplication");
    section.appendChild(availabilityMessage(id, analysisAvailability(state.data, id)));
    return [section];
  }
  if (id === "security") return [securitySection(state, fileIdx, navigate)];
  if (id === "architecture") {
    const sections: HTMLElement[] = [];
    const boundaries = boundariesSection(state, fileIdx, navigate);
    const cycle = cycleSection(state, file, fileIdx, navigate);
    if (boundaries) sections.push(boundaries);
    if (cycle) sections.push(cycle);
    const extra = findingsForFile(state.data, "architecture", fileIdx).filter(
      (finding) => finding.kind !== "boundary-violation" && finding.kind !== "circular-dependency",
    );
    if (extra.length > 0 || sections.length === 0) {
      sections.push(
        genericAnalysisSection(
          state,
          "architecture",
          fileIdx,
          new Set(["boundary-violation", "circular-dependency"]),
        ),
      );
    }
    return sections;
  }
  if (id === "health") {
    const health = genericAnalysisSection(state, "health", fileIdx);
    const complexity =
      analysisAvailability(state.data, "health").state === "complete"
        ? complexitySection(file)
        : null;
    return complexity ? [health, complexity] : [health];
  }
  return [genericAnalysisSection(state, id, fileIdx)];
};

const activeAnalysis = (
  state: AppState,
  model: FilePanelModel,
  file: VizFile,
  fileIdx: number,
  navigate: NavigateFn,
): HTMLElement => {
  const container = el("div", "active-signal");
  container.id = sectionId(model.active, false);
  container.appendChild(el("h2", undefined, SIGNAL_LABELS[model.active]));
  for (const section of analysisContent(state, model.active, file, fileIdx, navigate)) {
    container.appendChild(section);
  }
  return container;
};

const supportingAnalyses = (
  state: AppState,
  model: FilePanelModel,
  file: VizFile,
  fileIdx: number,
  navigate: NavigateFn,
): HTMLElement[] => {
  return model.signals.flatMap((signal): HTMLElement[] => {
    if (signal.active || (signal.count === 0 && signal.state === "complete")) return [];
    const details = el("details", "supporting-signals") as HTMLDetailsElement;
    details.id = sectionId(signal.id, true);
    const summary = el("summary");
    summary.appendChild(
      document.createTextNode(
        signal.id === "overview" ? signal.label : `${signal.label} ${formatCount(signal.count)}`,
      ),
    );
    details.appendChild(summary);
    for (const section of analysisContent(state, signal.id, file, fileIdx, navigate)) {
      details.appendChild(section);
    }
    return [details];
  });
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
    state.activeAnalysis,
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
  const model = filePanelModel(state, fileIdx);
  panel.appendChild(signalNavigator(model));
  panel.appendChild(activeAnalysis(state, model, file, fileIdx, navigate));
  for (const details of supportingAnalyses(state, model, file, fileIdx, navigate)) {
    panel.appendChild(details);
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
  fileIndex: number | null;
  finding?: GenericFindingView;
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
const usedByColumns: RankColumn[] = [
  { header: "used by", hint: "How many files import this one." },
];

/** The shared shape a ranked table renders from. */
interface RankView {
  rows: RankRow[];
  labelHead: string;
  columns: RankColumn[];
}

/** Keep large reports responsive while preserving the full payload for search,
 *  selection, and graph navigation. The final row reports the omitted count. */
export const MAX_RENDERED_RANK_ROWS = 500;

export const rankRowsForRender = (rows: RankRow[]): { rows: RankRow[]; truncated: number } => ({
  rows: rows.slice(0, MAX_RENDERED_RANK_ROWS),
  truncated: Math.max(0, rows.length - MAX_RENDERED_RANK_ROWS),
});

/** A lens's ranked findings plus its section title and empty-state copy. */
interface RankLensView extends RankView {
  title: string;
  empty: string;
}

const genericRankRows = (
  state: AppState,
  id: Exclude<AnalysisId, "unused" | "duplication" | "security">,
): RankRow[] =>
  findingsForAnalysis(state.data, id).map((finding): RankRow => {
    const pathIndex =
      finding.path === null ? -1 : state.data.files.findIndex((file) => file.path === finding.path);
    const indexedFile =
      finding.fileIndex !== null && state.data.files[finding.fileIndex] !== undefined
        ? finding.fileIndex
        : null;
    const fileIndex = indexedFile ?? (pathIndex >= 0 ? pathIndex : null);
    const path =
      finding.path ?? (fileIndex === null ? null : (state.data.files[fileIndex]?.path ?? null));
    const metric =
      finding.metrics.map(([label, value]) => `${label} ${value}`).join(", ") || finding.title;
    return {
      label: path ? basename(path) : finding.title,
      dir: path ? dirname(path) : "project",
      metric,
      cells: [{ value: finding.title, cls: finding.severity ? "sev-warn" : "" }],
      fileIndex,
      finding,
    };
  });

export const rankRowsFor = (state: AppState): RankLensView =>
  rankRowsForLens(state, (state.activeAnalysis ?? state.lens) as Lens);

const rankRowsForLens = (state: AppState, lens: Lens): RankLensView => {
  const files = state.data.files;
  switch (lens as string) {
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
    case "unused": {
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
    case "duplication": {
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
    case "architecture": {
      return {
        title: "Architecture findings",
        rows: genericRankRows(state, "architecture"),
        empty: "No architecture violations",
        labelHead: "location",
        columns: [
          {
            header: "finding",
            hint: "Boundary, policy, call, import-cycle, or re-export-cycle finding.",
          },
        ],
      };
    }
    case "health": {
      const rows = files
        .map((file, index) => ({ file, index, risk: healthRiskForFile(state.data, index) ?? 0 }))
        .filter(({ index }) => healthHasFindingForFile(state.data, index))
        .toSorted((left, right) => right.risk - left.risk)
        .map(({ file, index }): RankRow => {
          const findings = findingsForFile(state.data, "health", index);
          const fileHealth = state.data.health.files.find((entry) => entry.file === index);
          const metric = fileHealth
            ? `MI ${fileHealth.maintainability_index.toFixed(0)}, CRAP ${fileHealth.crap_max.toFixed(0)}`
            : (findings[0]?.title ?? "Review Health signals");
          return {
            label: basename(file.path),
            dir: dirname(file.path),
            metric,
            cells: [
              {
                value: metric,
                cls: findings.some((finding) => finding.severity) ? "sev-warn" : "",
              },
            ],
            fileIndex: index,
          };
        });
      return {
        title: "Files needing Health review",
        rows,
        empty: "No files need Health review",
        labelHead: "file",
        columns: [{ header: "health", hint: "Retained Health metrics for this file." }],
      };
    }
    case "security": {
      const priority = (severity: string): number => {
        switch (severity.toLowerCase()) {
          case "critical":
            return 4;
          case "high":
          case "error":
            return 3;
          case "medium":
          case "moderate":
            return 2;
          default:
            return 1;
        }
      };
      const rows = files
        .flatMap((file, index) =>
          securityCandidatesForFile(state.data, index).map((candidate) => ({
            file,
            index,
            candidate,
          })),
        )
        .toSorted(
          (left, right) => priority(right.candidate.severity) - priority(left.candidate.severity),
        )
        .map(({ file, index, candidate }) => ({
          label: basename(file.path),
          dir: dirname(file.path),
          metric: `${candidate.severity}: ${candidate.title}`,
          cells: [
            {
              value: candidate.severity,
              cls:
                priority(candidate.severity) >= 3
                  ? "sev-error"
                  : priority(candidate.severity) >= 2
                    ? "sev-warn"
                    : "sev-info",
            },
          ],
          fileIndex: index,
        }));
      return {
        title: "Static Security candidates",
        rows,
        empty: "No static Security candidates",
        labelHead: "file",
        columns: [{ header: "severity", hint: "Review priority, not proof of a vulnerability." }],
      };
    }
    case "dependencies":
    case "frameworks":
    case "styling":
    case "feature_flags":
    case "flags": {
      const wireId = lens as string;
      const id =
        wireId === "feature_flags"
          ? "flags"
          : (wireId as "dependencies" | "frameworks" | "styling" | "flags");
      const rows = genericRankRows(state, id);
      const noun = id === "flags" ? "uses" : "findings";
      return {
        title: `${SIGNAL_LABELS[id]} ${noun}`,
        rows,
        empty: `No ${SIGNAL_LABELS[id]} ${noun}`,
        labelHead: "file",
        columns: [{ header: "finding", hint: `${SIGNAL_LABELS[id]} analysis finding.` }],
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

const staticFileCell = (
  label: string,
  dir: string,
  budgetHint: number,
  finding?: GenericFindingView,
): HTMLElement => {
  const td = el("td", "col-file");
  if (!finding) {
    td.appendChild(rankLabelEl(label, dir, budgetHint));
    return td;
  }
  const details = el("details", "rank-details") as HTMLDetailsElement;
  const summary = el("summary");
  summary.appendChild(rankLabelEl(label, dir, budgetHint));
  details.appendChild(summary);
  details.appendChild(genericFindingEl(finding));
  td.appendChild(details);
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
  const { rows: renderedRows, truncated } = rankRowsForRender(rows);
  const hasBar = renderedRows.some((row) => row.bar !== undefined);
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
  renderedRows.forEach((row, index) => {
    const tr = el("tr");
    tr.appendChild(el("td", "col-rank", formatCount(index + 1)));
    tr.appendChild(
      row.fileIndex === null
        ? staticFileCell(row.label, row.dir ?? "", row.metric.length / 2, row.finding)
        : fileCell(row.label, row.dir ?? "", row.metric.length / 2, () => onPick(row)),
    );
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
  if (truncated > 0) {
    const tr = el("tr", "rank-truncated");
    const td = el(
      "td",
      "muted",
      `${formatCount(truncated)} additional rows not rendered`,
    ) as HTMLTableCellElement;
    td.colSpan = 2 + columns.length + (hasBar ? 1 : 0);
    tr.appendChild(td);
    tbody.appendChild(tr);
  }
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
    (row) => {
      if (row.fileIndex !== null) navigate(row.fileIndex);
    },
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
  const analysisId = activeAnalysisId(state);
  panel.replaceChildren();
  panel.classList.add("open");
  panel.setAttribute("aria-label", `${analysisId} findings`);

  const section = sectionEl(title);
  if (analysisId !== "overview") {
    const availability = analysisAvailability(state.data, analysisId);
    if (availability.state !== "complete") {
      section.appendChild(availabilityMessage(analysisId, availability));
      panel.appendChild(section);
      return;
    }
    if (availability.truncated && availability.truncated > 0) {
      section.appendChild(
        el("div", "muted", `${formatCount(availability.truncated)} ${availability.unit} not shown`),
      );
    }
  }
  if (analysisId === "health") {
    const summary = healthSummarySection(state);
    if (summary) panel.appendChild(summary);
  }
  if (analysisId === "frameworks") {
    const summary = frameworkSummarySection(state);
    if (summary) panel.appendChild(summary);
  }
  if (analysisId === "styling") {
    const summary = stylingSummarySection(state);
    if (summary) panel.appendChild(summary);
  }
  const analysisTruncated = (() => {
    switch (analysisId) {
      case "architecture":
        return state.data.architecture.findings_truncated;
      case "dependencies":
        return state.data.dependencies.findings_truncated;
      case "frameworks":
        return state.data.frameworks.findings_truncated;
      case "styling":
        return state.data.styling.findings_truncated;
      case "flags":
        return state.data.feature_flags.findings_truncated;
      default:
        return undefined;
    }
  })();
  if (analysisTruncated && analysisTruncated > 0) {
    section.appendChild(
      el("div", "muted", `${formatCount(analysisTruncated)} detail rows not shown`),
    );
  }
  if (rows.length === 0) {
    section.appendChild(el("div", "sev-ok", empty));
    panel.appendChild(section);
    if (analysisId === "security") panel.appendChild(securityCoverageSection(state));
    return;
  }
  section.appendChild(
    renderRankTable(state, { rows, labelHead, columns }, (row) => {
      if (row.clone !== undefined) {
        state.selectedClone = row.clone;
        refresh();
      } else {
        if (row.fileIndex !== null) navigate(row.fileIndex);
      }
    }),
  );
  panel.appendChild(section);
  if (analysisId === "health" && (state.data.health.findings_truncated ?? 0) > 0) {
    panel.appendChild(
      el(
        "div",
        "availability-state",
        `${formatCount(state.data.health.findings_truncated ?? 0)} Health findings not shown`,
      ),
    );
  }
  if (analysisId === "health" && (state.data.health.files_truncated ?? 0) > 0) {
    panel.appendChild(
      el(
        "div",
        "availability-state",
        `${formatCount(state.data.health.files_truncated ?? 0)} file score rows not shown`,
      ),
    );
  }
  if (analysisId === "security") panel.appendChild(securityCoverageSection(state));
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
      (row) => {
        if (row.fileIndex !== null) navigate(row.fileIndex);
      },
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
        (row) => {
          if (row.fileIndex !== null) navigate(row.fileIndex);
        },
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
