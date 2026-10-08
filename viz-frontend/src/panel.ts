import type { AppState } from "./state";
import type {
  Lens,
  SecondaryAnalysis,
  VizCloneGroup,
  RoadSelection,
  VizFile,
  VizHealthFile,
} from "./types";
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
  lensFindingLevel,
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
import { confidenceLabel, healthReason, securityCategoryLabel, severityRank } from "./explain";
import { LENSES } from "./lenses";
import { getGVS } from "./graph/shared";

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

/** A compact magnitude meter: a filled run over a quiet track. The same
 *  motif the per-function complexity bar uses, generalized so ranked lists,
 *  blast-radius facts, and search totals can reuse it. The fill grows in
 *  from the left when the panel renders (see `.bar` in styles.css). */
const meterBar = (value: number, max: number, tone: MeterTone): HTMLElement => {
  const ratio = max > 0 ? Math.max(0, Math.min(1, value / max)) : 0;
  const bar = el("span", "bar");
  bar.setAttribute("aria-hidden", "true");
  const fillClass =
    tone === "error" ? "fill-error" : tone === "neutral" ? "fill-neutral" : "fill-warn";
  const fill = el("span", `bar-fill ${fillClass}`);
  // A non-zero value always shows a sliver, so it never reads as empty.
  fill.style.setProperty("--fill", String(value > 0 ? Math.max(0.06, ratio) : 0));
  bar.appendChild(fill);
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
  const rank = severityRank(candidate.severity);
  const article = el("article", `security-candidate lvl-${rank >= 3 ? 2 : rank >= 2 ? 1 : 0}`);
  // Lead with the consequence in plain words; the rule id moves to details.
  const heading = el(
    "h4",
    "finding-title",
    securityCategoryLabel(candidate.category, candidate.title),
  );
  if (candidate.line !== null)
    heading.appendChild(el("span", "finding-line", `line ${candidate.line}`));
  article.appendChild(heading);
  const badges = el("div", "finding-badges");
  badges.appendChild(
    el(
      "span",
      `badge-sev ${rank >= 3 ? "sev-error" : rank >= 2 ? "sev-warn" : "sev-info"}`,
      candidate.severity,
    ),
  );
  if (candidate.confidence) {
    badges.appendChild(el("span", "badge-soft", confidenceLabel(candidate.confidence)));
  }
  article.appendChild(badges);
  if (candidate.source || candidate.sink) {
    const flow = el("div", "taint-pill");
    flow.appendChild(el("span", "tp-end", candidate.source ?? "input"));
    flow.appendChild(el("span", "tp-arrow", "→"));
    flow.appendChild(el("span", "tp-end tp-sink", candidate.sink ?? "sink"));
    article.appendChild(flow);
  }
  if (candidate.evidence) article.appendChild(el("p", "finding-evidence", candidate.evidence));
  if (candidate.verificationPrompt) {
    const check = el("p", "finding-check");
    check.appendChild(el("strong", undefined, "Check: "));
    check.appendChild(document.createTextNode(candidate.verificationPrompt));
    article.appendChild(check);
  }
  const facts: KvPair[] = [];
  if (candidate.category) facts.push(["Rule", candidate.category]);
  if (candidate.cwe) facts.push(["CWE", candidate.cwe]);
  if (candidate.line !== null) {
    facts.push([
      "Location",
      `${candidate.path}:${candidate.line}${candidate.column === null ? "" : `:${candidate.column}`}`,
    ]);
  }
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
  const more = el("details", "finding-more") as HTMLDetailsElement;
  more.appendChild(el("summary", undefined, "Evidence and trace"));
  if (facts.length > 0) more.appendChild(kvEl(facts));
  if (candidate.trace.length > 0) {
    const trace = el("ol", "trace-list");
    for (const step of candidate.trace) trace.appendChild(el("li", undefined, step));
    more.appendChild(trace);
  }
  if (candidate.taintFlow) {
    more.appendChild(el("p", "finding-evidence", `Taint flow: ${candidate.taintFlow}`));
  }
  if (candidate.observedControls.length > 0) {
    const controls = el("div", "observed-controls");
    controls.appendChild(el("strong", undefined, "Observed controls"));
    const list = el("ul");
    for (const control of candidate.observedControls)
      list.appendChild(el("li", undefined, control));
    controls.appendChild(list);
    more.appendChild(controls);
  }
  article.appendChild(more);
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
        "Files that import this file, directly or indirectly.",
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

/** Plain next step for a Health action id, falling back to its own text. */
const healthActionText = (action: FindingActionView): string | null => {
  const key = action.label.toLowerCase();
  if (key.startsWith("refactor")) return "Split it into smaller functions";
  if (key.startsWith("add tests")) return "Add tests";
  if (key.startsWith("increase coverage")) return "Add tests for the branches without coverage";
  if (key.startsWith("suppress")) return null;
  return action.description ?? null;
};

/** A metric tile: value, unit, and a tone that says whether it is a problem. */
const metricTile = (
  label: string,
  value: string,
  tone: "error" | "warn" | "ok" | "",
): HTMLElement => {
  const tile = el("div", `metric-tile${tone ? ` tone-${tone}` : ""}`);
  tile.appendChild(el("span", "mt-value", value));
  tile.appendChild(el("span", "mt-label", label));
  return tile;
};

/** Metric chips of a flagged function, keyed by the lower-cased fact label. */
const FUNCTION_METRIC_CHIPS: ReadonlyArray<{
  fact: string;
  text: (value: string) => string;
  hint: string;
}> = [
  {
    fact: "cyclomatic",
    text: (value) => `${value} branches`,
    hint: "Cyclomatic complexity: the number of paths through the function.",
  },
  {
    fact: "cognitive",
    text: (value) => `difficulty ${value}`,
    hint: "Cognitive complexity: how hard the function is to follow.",
  },
  {
    fact: "line count",
    text: (value) => `${value} lines`,
    hint: "Lines of code in the function.",
  },
];

/** Coverage chip per coverage tier; a fully covered function gets none. */
const COVERAGE_CHIPS = new Map<string, { text: string; hint: string }>([
  ["none", { text: "no tests", hint: "No test covers this function." }],
  ["partial", { text: "some tests", hint: "Tests cover part of this function." }],
]);

/** The chip row of one flagged function, or null when no fact applies. */
const functionChips = (facts: Map<string, string>): HTMLElement | null => {
  const chips = el("div", "fn-chips");
  const chip = (text: string, hint: string): void => {
    const node = el("span", "fn-chip", text);
    node.dataset.tip = hint;
    chips.appendChild(node);
  };
  for (const spec of FUNCTION_METRIC_CHIPS) {
    const value = facts.get(spec.fact);
    if (value) chip(spec.text(value), spec.hint);
  }
  const coverage = COVERAGE_CHIPS.get(facts.get("coverage tier") ?? "");
  if (coverage) chip(coverage.text, coverage.hint);
  return chips.childNodes.length > 0 ? chips : null;
};

/** The collapsed suppress hint of a finding, or null without a command. */
const suppressDetails = (finding: GenericFindingView): HTMLElement | null => {
  const suppress = finding.actions.find((action) =>
    action.label.toLowerCase().startsWith("suppress"),
  );
  if (!suppress?.command) return null;
  const more = el("details", "finding-more") as HTMLDetailsElement;
  more.appendChild(el("summary", undefined, "Suppress"));
  more.appendChild(commandHint("comment", suppress.command));
  return more;
};

/** Card level of a severity rank: 2 severe, 1 mild, 0 none. */
const rankLevel = (rank: number): number => {
  if (rank >= 3) return 2;
  return rank >= 2 ? 1 : 0;
};

/** One function Health flagged: where it is, why, and what to do. */
const healthFunctionEl = (finding: GenericFindingView): HTMLElement => {
  const facts = new Map(finding.metrics.map(([label, value]) => [label.toLowerCase(), value]));
  const rank = severityRank(finding.severity ?? "");
  const card = el("article", `fn-card lvl-${rankLevel(rank)}`);
  const head = el("div", "fn-head");
  head.appendChild(el("span", "fn-title", facts.get("name") ?? finding.title));
  if (finding.line !== null) head.appendChild(el("span", "finding-line", `line ${finding.line}`));
  card.appendChild(head);
  const chips = functionChips(facts);
  if (chips) card.appendChild(chips);
  const steps = finding.actions
    .map(healthActionText)
    .filter((text): text is string => text !== null);
  if (steps.length > 0) {
    card.appendChild(el("p", "fn-fix", [...new Set(steps)].join(", or ")));
  }
  const more = suppressDetails(finding);
  if (more) card.appendChild(more);
  return card;
};

/** Tone of a value where lower is worse. */
const lowIsBadTone = (value: number, error: number, warn: number): "error" | "warn" | "ok" => {
  if (value < error) return "error";
  return value < warn ? "warn" : "ok";
};

/** Tone of a value where higher is worse. */
const highIsBadTone = (value: number, error: number, warn: number): "error" | "warn" | "ok" => {
  if (value >= error) return "error";
  return value >= warn ? "warn" : "ok";
};

/** The three numbers that say how bad a file's health is. */
const healthMetricTiles = (file: VizFile, fileHealth: VizHealthFile): HTMLElement => {
  const mi = fileHealth.maintainability_index;
  const risk = fileHealth.crap_max;
  const tiles = el("div", "metric-tiles");
  tiles.appendChild(metricTile("maintainability", mi.toFixed(0), lowIsBadTone(mi, 50, 70)));
  tiles.appendChild(
    metricTile("change risk", formatCount(Math.round(risk)), highIsBadTone(risk, 30, 10)),
  );
  tiles.appendChild(metricTile("imported by", formatCount(file.importer_count), ""));
  return tiles;
};

/** The "Functions to fix" list of a file, appended to `section`. */
const appendHealthFunctions = (section: HTMLElement, functions: GenericFindingView[]): void => {
  if (functions.length === 0) return;
  section.appendChild(el("h4", "sub-head", `Functions to fix (${formatCount(functions.length)})`));
  for (const finding of functions) section.appendChild(healthFunctionEl(finding));
};

/**
 * The Health tab for one file: why the file is listed, three numbers that
 * say how bad it is, then each function to fix with its next step. Raw
 * analyzer fields stay out; the function table below keeps the detail.
 */
const healthFileSection = (state: AppState, file: VizFile, fileIdx: number): HTMLElement => {
  const availability = analysisAvailability(state.data, "health");
  const section = sectionEl("Health");
  if (availability.state !== "complete") {
    section.appendChild(availabilityMessage("health", availability));
    return section;
  }
  const fileHealth = state.data.health.files.find((entry) => entry.file === fileIdx);
  const findings = findingsForFile(state.data, "health", fileIdx);
  const functions = findings.filter((finding) =>
    finding.metrics.some(([label]) => label.toLowerCase() === "name"),
  );
  const other = findings.filter(
    (finding) => !functions.includes(finding) && finding.kind !== "file-health",
  );
  const fallback = findings.length > 0 ? "Health threshold exceeded" : "No Health findings";
  section.appendChild(el("p", "health-reason", healthReason(fileHealth, fallback)));
  if (fileHealth) section.appendChild(healthMetricTiles(file, fileHealth));
  appendHealthFunctions(section, functions);
  for (const finding of other) section.appendChild(genericFindingEl(finding));
  return section;
};

/**
 * The first thing the Overview tab says about a file: every lens that
 * flags it, worst first, with the plain reason. Each row opens that lens's
 * section in the panel.
 */
const fileFindingsSummary = (state: AppState, fileIdx: number): HTMLElement => {
  const section = sectionEl("Findings");
  const found: Array<{ lens: (typeof LENSES)[number]; row: RankRow }> = [];
  for (const lens of LENSES) {
    if (lens.id === "overview") continue;
    if (analysisAvailability(state.data, lens.id as AnalysisId).state !== "complete") continue;
    const row = rankRowsForLens(state, lens.id).rows.find(
      (candidate) => candidate.fileIndex === fileIdx,
    );
    if (row) found.push({ lens, row });
  }
  if (found.length === 0) {
    section.appendChild(el("p", "sev-ok", "No findings in this file"));
    return section;
  }
  found.sort((left, right) => (right.row.level ?? 1) - (left.row.level ?? 1));
  const list = el("ul", "file-findings");
  for (const { lens, row } of found) {
    const item = el("li", `ff-row lvl-${row.level ?? 1}`);
    const btn = el("button", "ff-btn") as HTMLButtonElement;
    btn.type = "button";
    btn.appendChild(el("span", "ff-lens", lens.name));
    btn.appendChild(el("span", "ff-why", row.why ?? row.metric));
    btn.addEventListener("click", () => {
      const target =
        document.getElementById(sectionId(lens.id as AnalysisId, true)) ??
        document.getElementById(sectionId(lens.id as AnalysisId, false));
      if (target instanceof HTMLDetailsElement) target.open = true;
      target?.scrollIntoView({ block: "start", behavior: "smooth" });
    });
    item.appendChild(btn);
    list.appendChild(item);
  }
  section.appendChild(list);
  return section;
};

/**
 * "3 of 61" with previous and next buttons, when the open file is in the
 * active lens's list. Triage then walks the list from the file view (also
 * with the j and k keys) instead of going back to the list each time.
 */
const findingStepper = (
  state: AppState,
  fileIdx: number,
  navigate: NavigateFn,
): HTMLElement | null => {
  if (state.lens === "overview" && state.activeAnalysis === null) return null;
  const order = rankRowsFor(state)
    .rows.toSorted((left, right) => (right.level ?? 1) - (left.level ?? 1))
    .flatMap((row) => (row.fileIndex === null ? [] : [row.fileIndex]));
  const files = [...new Set(order)];
  const position = files.indexOf(fileIdx);
  if (position < 0 || files.length < 2) return null;
  const bar = el("nav", "finding-stepper");
  bar.setAttribute("aria-label", "Step through findings");
  const step = (cls: string, label: string, text: string, target: number | undefined): void => {
    const btn = el("button", `step-btn ${cls}`, text) as HTMLButtonElement;
    btn.type = "button";
    btn.setAttribute("aria-label", label);
    if (target === undefined) btn.disabled = true;
    else btn.addEventListener("click", () => navigate(target));
    bar.appendChild(btn);
  };
  step("step-prev", "Previous finding (k)", "‹", files[position - 1]);
  const where = el("span", "step-pos");
  where.appendChild(el("b", undefined, formatCount(position + 1)));
  where.appendChild(
    document.createTextNode(` of ${formatCount(files.length)} in ${lensLabel(state)}`),
  );
  bar.appendChild(where);
  step("step-next", "Next finding (j)", "›", files[position + 1]);
  return bar;
};

/** The name of the active lens or secondary analysis, for the stepper. */
const lensLabel = (state: AppState): string =>
  state.activeAnalysis !== null
    ? SIGNAL_LABELS[analysisIdForSecondary(state.activeAnalysis)]
    : (LENSES.find((lens) => lens.id === state.lens)?.name ?? state.lens);

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
  const section = sectionEl("Security candidates");
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
    return [
      fileFindingsSummary(state, fileIdx),
      factsSection(state, file, fileIdx),
      ...connectionSections(state, fileIdx, navigate),
    ];
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
    const health = healthFileSection(state, file, fileIdx);
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
  // The tab row above names the active signal; the sections carry their own
  // headings, so a second label here only repeats it.
  container.setAttribute("aria-label", SIGNAL_LABELS[model.active]);
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
  // Census code names: the name leads, its path sits under it.
  fileBox.appendChild(el("div", "name", basename(file.path)));
  const dir = dirname(file.path);
  if (dir) fileBox.appendChild(el("div", "dir", `${dir}/`));
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
    // The architecture panel lists the graph's folder loops, which change
    // with the grouping and exist only once the graph is built.
    state.view,
    getGVS(state).clusterMode,
    getGVS(state).initialized,
  ].join("|");

/** The panel without a selected file: clone, road, search, or lens list. */
const renderUnselectedPanel = (
  state: AppState,
  panel: HTMLElement,
  navigate: NavigateFn,
  close: () => void,
  refresh: () => void,
): void => {
  if (state.selectedClone !== null) {
    renderClonePanel(state, panel, navigate, refresh);
    return;
  }
  if (state.selectedRoad !== null) {
    renderRoadPanel(state, panel, navigate, close);
    return;
  }
  if (state.search.trim() !== "") {
    // An active query owns the sidebar: the matched files and their
    // combined blast radius, not the lens list they'd otherwise see.
    renderSearchPanel(state, panel, navigate);
    return;
  }
  // Nothing selected: every lens shows a ranked list. Finding lenses
  // rank worst-first; overview shows the triage cards and the most
  // imported files.
  renderLensPanel(state, panel, navigate, refresh);
};

/** The detail panel of one selected file. */
const renderFilePanel = (
  state: AppState,
  panel: HTMLElement,
  fileIdx: number,
  navigate: NavigateFn,
  close: () => void,
): void => {
  const file = state.data.files[fileIdx];
  panel.replaceChildren();
  panel.classList.add("open");
  panel.setAttribute("aria-label", "file details");
  panel.appendChild(fileHead(file, close));
  const stepper = findingStepper(state, fileIdx, navigate);
  if (stepper) panel.appendChild(stepper);
  const model = filePanelModel(state, fileIdx);
  panel.appendChild(signalNavigator(model));
  panel.appendChild(activeAnalysis(state, model, file, fileIdx, navigate));
  for (const details of supportingAnalyses(state, model, file, fileIdx, navigate)) {
    panel.appendChild(details);
  }
};

export const renderPanel = (
  state: AppState,
  panel: HTMLElement,
  navigate: NavigateFn,
  close: () => void,
  refresh: () => void,
): void => {
  if (state.selected === null) {
    renderUnselectedPanel(state, panel, navigate, close, refresh);
    return;
  }
  renderFilePanel(state, panel, state.selected, navigate, close);
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
  /** Plain-language reason the row is listed, shown under the path. */
  why?: string;
  /** Severity tier: 2 = high, 1 = medium, 0 = low. */
  level?: 0 | 1 | 2;
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
  { header: "Used by", hint: "How many files import this one." },
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
      why: finding.detail ?? finding.title,
      level: finding.severity ? (severityRank(finding.severity) >= 3 ? 2 : 1) : 1,
    };
  });

/** Names the unused exports so the row says what to delete, not just how many. */
const unusedExportsReason = (file: VizFile): string => {
  const names = file.unused_exports ?? [];
  if (names.length === 0) {
    return `${formatCount(file.unused_export_count)} exports nothing imports`;
  }
  const shown = names.slice(0, 3).join(", ");
  const rest = file.unused_export_count - Math.min(3, names.length);
  return `Unused: ${shown}${rest > 0 ? ` +${formatCount(rest)} more` : ""}`;
};

/** Where the copies of a clone group live, by file name. */
const cloneReason = (state: AppState, group: VizCloneGroup): string => {
  const names = [
    ...new Set(group.instances.map((instance) => basename(state.data.files[instance.file].path))),
  ];
  const copies = `${formatCount(group.instances.length)} copies`;
  if (names.length === 1) return `${copies} inside this file`;
  return `${copies}: ${names.slice(0, 3).join(", ")}${names.length > 3 ? " …" : ""}`;
};

export const rankRowsFor = (state: AppState): RankLensView =>
  rankRowsForLens(state, (state.activeAnalysis ?? state.lens) as Lens);

const rankRowsForLens = (state: AppState, lens: Lens): RankLensView => {
  const files = state.data.files;
  switch (lens as string) {
    case "overview": {
      // Files ranked by how many files import them. Reuses the shared
      // used-by row shape (fileRankRows).
      const ranked = files
        .map((file, index) => ({ file, index }))
        .filter(({ file }) => file.importer_count > 0)
        .toSorted((left, right) => right.file.importer_count - left.file.importer_count)
        .map(({ index }) => index);
      const rows = fileRankRows(state, ranked);
      return {
        title: "Most imported files",
        rows,
        empty: "No file is imported by another file",
        labelHead: "File",
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
          why: "No file imports this file",
          level: 2,
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
          why: unusedExportsReason(file),
          level: 1,
        });
      }
      return {
        title: "Unused files and exports",
        rows,
        empty: "Nothing is unreachable",
        labelHead: "File",
        columns: [
          {
            header: "Unused",
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
          why: cloneReason(state, group),
          level: (group.lines >= 30 || group.instances.length >= 3 ? 2 : 1) as 0 | 1 | 2,
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
        labelHead: "Block",
        columns: [{ header: "Lines", hint: "Number of duplicated lines in the block." }],
      };
    }
    case "architecture": {
      return {
        title: "Architecture findings",
        rows: genericRankRows(state, "architecture"),
        empty: "No architecture violations",
        labelHead: "Location",
        columns: [
          {
            header: "Finding",
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
          const level = lensFindingLevel("health", state.index, file, index);
          const risk = fileHealth ? fileHealth.crap_max : null;
          return {
            label: basename(file.path),
            dir: dirname(file.path),
            metric,
            cells: [
              {
                value: risk === null ? "review" : formatCount(Math.round(risk)),
                cls: level >= 2 ? "sev-error" : level === 1 ? "sev-warn" : "muted",
              },
            ],
            fileIndex: index,
            why: healthReason(fileHealth, findings[0]?.title ?? "Health threshold exceeded"),
            level,
          };
        });
      return {
        title: "Health findings",
        rows,
        empty: "No files need Health review",
        labelHead: "File",
        columns: [
          {
            header: "Risk",
            hint: "Change risk: complexity weighted by missing tests (CRAP). Above 30 is high.",
          },
        ],
      };
    }
    case "security": {
      const rows = files
        .map((file, index) => ({
          file,
          index,
          candidates: securityCandidatesForFile(state.data, index),
        }))
        .filter(({ candidates }) => candidates.length > 0)
        .map(({ file, index, candidates }) => {
          const top = Math.max(...candidates.map((candidate) => severityRank(candidate.severity)));
          const labels = [
            ...new Set(
              candidates
                .toSorted(
                  (left, right) => severityRank(right.severity) - severityRank(left.severity),
                )
                .map((candidate) => securityCategoryLabel(candidate.category, candidate.title)),
            ),
          ];
          const topSeverity =
            candidates.find((candidate) => severityRank(candidate.severity) === top)?.severity ??
            "low";
          return { file, index, candidates, top, labels, topSeverity };
        })
        .toSorted(
          (left, right) => right.top - left.top || right.candidates.length - left.candidates.length,
        )
        .map(({ file, index, candidates, top, labels, topSeverity }): RankRow => ({
          label: basename(file.path),
          dir: dirname(file.path),
          metric: `${topSeverity}: ${labels[0]}`,
          cells: [
            {
              value: candidates.length === 1 ? topSeverity : `${formatCount(candidates.length)}×`,
              cls: top >= 3 ? "sev-error" : top >= 2 ? "sev-warn" : "sev-info",
            },
          ],
          fileIndex: index,
          why:
            labels.length > 2
              ? `${labels.slice(0, 2).join(" · ")} · +${labels.length - 2} more`
              : labels.join(" · "),
          level: top >= 3 ? 2 : top >= 2 ? 1 : 0,
        }));
      return {
        title: "Security candidates",
        rows,
        empty: "No static Security candidates",
        labelHead: "File",
        columns: [
          {
            header: "Candidates",
            hint: "Static candidates in this file. Review priority, not proof of a vulnerability.",
          },
        ],
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
        labelHead: "File",
        columns: [{ header: "Finding", hint: `${SIGNAL_LABELS[id]} analysis finding.` }],
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
const rankLabelEl = (label: string, dir: string, _budgetHint: number): HTMLElement => {
  const labelBox = el("span", "rank-label");
  const full = `${dir ? `${dir}/` : ""}${label}`;
  const nameSpan = el("span", "rank-name", label);
  nameSpan.title = full;
  labelBox.appendChild(nameSpan);
  if (dir) {
    // The folder trails the name and gives way first; CSS ellipsizes it.
    const dirSpan = el("span", "muted rank-dir", `${dir}/`);
    dirSpan.title = full;
    labelBox.appendChild(dirSpan);
  }
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
    { rows: fileRankRows(state, indices), labelHead: "File", columns: usedByColumns },
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

/** What each finding lens means for the reader, in one sentence. */
const LENS_PURPOSE: Partial<Record<Lens, string>> = {
  unused: "Files and exports that no code imports.",
  duplication:
    "Code blocks that occur in more than one place. A fix in one copy does not change the other copies.",
  architecture: "Imports that your boundary rules forbid, and import cycles.",
  health:
    "Files with complex code, low test coverage, or many importers. A change to these files is more likely to cause a bug.",
  security:
    "Places where external input reaches a sensitive call. Static analysis cannot confirm a vulnerability, so examine each one.",
};

/** What one row counts, for lenses whose rows are not files. */
const ROW_NOUN: Partial<Record<Lens, [string, string]>> = {
  duplication: ["duplicated block", "duplicated blocks"],
  architecture: ["finding", "findings"],
};

const LEVEL_GROUPS: ReadonlyArray<{ level: 0 | 1 | 2; title: string; tone: string }> = [
  { level: 2, title: "High", tone: "error" },
  { level: 1, title: "Medium", tone: "warn" },
  { level: 0, title: "Low", tone: "neutral" },
];

/** Import rows between two folders, grouped by what is wrong with them. */
const ROAD_GROUPS: ReadonlyArray<{ level: 0 | 1 | 2; title: string; tone: string }> = [
  { level: 2, title: "Forbidden", tone: "error" },
  { level: 1, title: "In a cycle", tone: "warn" },
  { level: 0, title: "Imports", tone: "neutral" },
];

/** Rows rendered per severity group before a "show more" control. */
const GROUP_PAGE = 30;

const levelCounts = (rows: RankRow[]): [number, number, number] => {
  const counts: [number, number, number] = [0, 0, 0];
  for (const row of rows) counts[row.level ?? 1] += 1;
  return counts;
};

/** A proportional three-tone strip: how the findings split by severity. */
const severityStrip = (counts: [number, number, number]): HTMLElement => {
  const strip = el("div", "sev-strip");
  const total = counts[0] + counts[1] + counts[2];
  for (const group of LEVEL_GROUPS) {
    const share = total === 0 ? 0 : counts[group.level] / total;
    if (share === 0) continue;
    const seg = el("span", `sev-seg tone-${group.tone}`);
    seg.style.setProperty("--share", String(share));
    strip.appendChild(seg);
  }
  return strip;
};

/** The lens header: what it measures, how much there is, how bad it is. */
const lensSummaryEl = (state: AppState, rows: RankRow[]): HTMLElement => {
  const box = el("section", "lens-brief");
  const counts = levelCounts(rows);
  const head = el("div", "brief-head");
  head.appendChild(el("span", "brief-num", formatCount(rows.length)));
  const [one, many] = ROW_NOUN[state.lens] ?? ["file", "files"];
  head.appendChild(el("span", "brief-unit", rows.length === 1 ? one : many));
  box.appendChild(head);
  const purpose = LENS_PURPOSE[state.lens];
  if (purpose) box.appendChild(el("p", "brief-purpose", purpose));
  if (rows.length > 0) {
    box.appendChild(severityStrip(counts));
    const legend = el("div", "brief-legend");
    for (const group of LEVEL_GROUPS) {
      if (counts[group.level] === 0) continue;
      const item = el("span", `brief-key tone-${group.tone}`);
      item.appendChild(el("b", "", formatCount(counts[group.level])));
      item.appendChild(document.createTextNode(` ${group.title.toLowerCase()}`));
      legend.appendChild(item);
    }
    box.appendChild(legend);
  }
  return box;
};

/** One finding: file name first and whole, its folder, and why it is listed. */
const findingRowEl = (row: RankRow, onPick: (row: RankRow) => void): HTMLElement => {
  const li = el("li", `finding-row lvl-${row.level ?? 1}`);
  const btn = el("button", "fr-btn") as HTMLButtonElement;
  btn.type = "button";
  const text = el("span", "fr-text");
  const name = el("span", "fr-name", row.label);
  text.appendChild(name);
  if (row.dir) {
    // RTL keeps the nearest folder visible when the path is long; the
    // inner ltr span stops the slashes from being reordered.
    const dir = el("span", "fr-dir");
    const inner = el("span", "", `${row.dir}/`);
    inner.dir = "ltr";
    dir.appendChild(inner);
    dir.title = `${row.dir}/${row.label}`;
    text.appendChild(dir);
  }
  if (row.why) text.appendChild(el("span", "fr-why", row.why));
  btn.appendChild(text);
  const value = el("span", "fr-value");
  for (const cell of row.cells) value.appendChild(el("span", `fr-num ${cell.cls}`, cell.value));
  if (row.bar) value.appendChild(meterBar(row.bar.value, row.bar.max, row.bar.tone));
  btn.appendChild(value);
  if (row.fileIndex === null && row.clone === undefined) {
    btn.disabled = true;
  } else {
    btn.addEventListener("click", () => onPick(row));
  }
  li.appendChild(btn);
  return li;
};

/**
 * Findings grouped by severity tier. Each group shows a page of rows and a
 * control for the rest, so a 180-file list still opens on what matters.
 */
const renderFindingList = (
  rows: RankRow[],
  columns: RankColumn[],
  onPick: (row: RankRow) => void,
  groups: ReadonlyArray<{ level: 0 | 1 | 2; title: string; tone: string }> = LEVEL_GROUPS,
): HTMLElement => {
  const box = el("div", "finding-groups");
  const unit = columns[0];
  for (const group of groups) {
    const members = rows.filter((row) => (row.level ?? 1) === group.level);
    if (members.length === 0) continue;
    const section = el("section", `finding-group tone-${group.tone}`);
    const header = el("h4", "fg-head");
    header.appendChild(el("span", "fg-title", group.title));
    header.appendChild(el("span", "fg-count", formatCount(members.length)));
    if (unit) {
      const colHead = el("span", "fg-col", unit.header);
      colHead.dataset.tip = unit.hint;
      header.appendChild(colHead);
    }
    section.appendChild(header);
    // A reason every row shares is said once, under the group header.
    const reasons = new Set(members.map((row) => row.why ?? ""));
    const sharedWhy = members.length > 1 && reasons.size === 1 ? members[0].why : undefined;
    if (sharedWhy) section.appendChild(el("p", "fg-why", sharedWhy));
    const list = el("ol", "finding-list");
    const renderPage = (from: number): void => {
      for (const row of members.slice(from, from + GROUP_PAGE)) {
        list.appendChild(findingRowEl(sharedWhy ? { ...row, why: undefined } : row, onPick));
      }
      const remaining = members.length - (from + GROUP_PAGE);
      if (remaining <= 0) return;
      const more = el("button", "fg-more", `Show ${formatCount(remaining)} more`);
      (more as HTMLButtonElement).type = "button";
      more.addEventListener("click", () => {
        more.remove();
        renderPage(from + GROUP_PAGE);
      });
      section.appendChild(more);
    };
    section.appendChild(list);
    renderPage(0);
    box.appendChild(section);
  }
  return box;
};

type LensDef = (typeof LENSES)[number];

/** The health grade and score badge at the top of the triage. */
const triageScore = (grade: string | undefined, value: number): HTMLElement => {
  const score = el("div", "triage-score");
  score.appendChild(el("span", `grade grade-${(grade ?? "").toLowerCase()}`, grade ?? ""));
  const text = el("div", "score-text");
  text.appendChild(el("span", "score-num", `${value.toFixed(0)}/100`));
  text.appendChild(el("span", "score-label", "Health score"));
  score.appendChild(text);
  return score;
};

/**
 * The unit after a triage count. The tab counts findings; this card counts
 * files. When the two differ (several security candidates in one file),
 * say both so the numbers do not seem to disagree.
 */
const triageUnit = (state: AppState, lens: LensDef, rowCount: number): string => {
  const [one, many] = ROW_NOUN[lens.id] ?? ["file", "files"];
  const tabCount = lens.count(state);
  const unit = rowCount === 1 ? one : many;
  if (!tabCount || tabCount.value === rowCount) return unit;
  return `${unit}, ${formatCount(tabCount.value)} ${tabCount.unit}`;
};

/** The headline figure of a triage card: off, clean, or the counts. */
const triageFigure = (
  state: AppState,
  lens: LensDef,
  analyzed: boolean,
  rows: RankRow[],
  counts: [number, number, number],
): HTMLElement => {
  const figure = el("span", "tc-figure");
  if (!analyzed) {
    figure.appendChild(el("span", "tc-off", "Not analyzed"));
    return figure;
  }
  if (rows.length === 0) {
    figure.appendChild(el("span", "tc-ok", "No findings"));
    return figure;
  }
  figure.appendChild(el("span", "tc-num", formatCount(rows.length)));
  figure.appendChild(el("span", "tc-unit", triageUnit(state, lens, rows.length)));
  if (counts[2] > 0) figure.appendChild(el("span", "tc-severe", `${formatCount(counts[2])} high`));
  return figure;
};

/** The worst file of a triage card with its reason, or null. */
const triageSample = (rows: RankRow[]): HTMLElement | null => {
  const worst = rows.find((row) => row.fileIndex !== null);
  if (!worst) return null;
  const sample = el("span", "tc-sample");
  sample.appendChild(el("span", "tc-file", worst.label));
  if (worst.why) sample.appendChild(el("span", "tc-why", worst.why));
  return sample;
};

/** One triage card: size, severity split and worst file of a lens. */
const triageCard = (state: AppState, lens: LensDef): HTMLElement => {
  const analyzed = analysisAvailability(state.data, lens.id as AnalysisId).state === "complete";
  const rows = analyzed ? rankRowsForLens(state, lens.id).rows : [];
  const counts = levelCounts(rows);
  const card = el("button", `triage-card${rows.length === 0 ? " is-clean" : ""}`);
  (card as HTMLButtonElement).type = "button";
  const top = el("span", "tc-top");
  top.appendChild(el("span", "tc-name", lens.name));
  top.appendChild(el("kbd", "tc-key", lens.shortcut));
  card.appendChild(top);
  card.appendChild(triageFigure(state, lens, analyzed, rows, counts));
  if (rows.length > 0) card.appendChild(severityStrip(counts));
  const sample = triageSample(rows);
  if (sample) card.appendChild(sample);
  card.addEventListener("click", () => {
    card.dispatchEvent(new CustomEvent("fallow:lens", { detail: lens.id, bubbles: true }));
  });
  return card;
};

/**
 * The overview's first question: how is this codebase doing, and where
 * should I start? One card per finding lens with its size, severity split,
 * and the worst files, each card opening its lens.
 */
const triageSection = (state: AppState): HTMLElement => {
  const section = el("section", "triage");
  const health = state.data.health;
  if (health.score !== undefined) section.appendChild(triageScore(health.grade, health.score));
  const cards = el("div", "triage-cards");
  for (const lens of LENSES) {
    if (lens.id !== "overview") cards.appendChild(triageCard(state, lens));
  }
  section.appendChild(cards);
  return section;
};

/**
 * The folders the graph outlines as an import loop. They are not rule
 * violations, so the findings list stays empty; this section keeps the
 * panel in step with the outlines on the map.
 */
const folderLoopsSection = (state: AppState): HTMLElement | null => {
  if (state.view !== "graph") return null;
  const gvs = getGVS(state);
  const loops = gvs.clusters.filter((cluster) => cluster.tangle && cluster.indices.length > 1);
  if (loops.length === 0) return null;
  const section = sectionEl(`Folders in an import loop (${formatCount(loops.length)})`);
  section.appendChild(
    el(
      "p",
      "brief-purpose",
      "These folders import each other, directly or through other folders. No rule forbids it, but a change in one can reach all of them, and they are hard to move or split on their own.",
    ),
  );
  const list = el("ul", "loop-list");
  for (const cluster of loops.toSorted(
    (left, right) => right.indices.length - left.indices.length,
  )) {
    const item = el("li");
    item.appendChild(el("span", "loop-name", cluster.key));
    item.appendChild(el("span", "muted", `${formatCount(cluster.indices.length)} files`));
    list.appendChild(item);
  }
  section.appendChild(list);
  return section;
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

  if (analysisId === "overview") panel.appendChild(triageSection(state));
  else if (state.activeAnalysis === null) panel.appendChild(lensSummaryEl(state, rows));
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
    if (analysisId !== "overview" && state.activeAnalysis === null) {
      section.querySelector(":scope > h3")?.remove();
    }
    section.appendChild(el("div", "sev-ok", empty));
    panel.appendChild(section);
    if (analysisId === "architecture") {
      const loops = folderLoopsSection(state);
      if (loops) panel.appendChild(loops);
    }
    if (analysisId === "security") panel.appendChild(securityCoverageSection(state));
    return;
  }
  const pick = (row: RankRow): void => {
    if (row.clone !== undefined) {
      state.selectedClone = row.clone;
      refresh();
    } else if (row.fileIndex !== null) {
      navigate(row.fileIndex);
    }
  };
  if (analysisId === "overview") {
    section.appendChild(renderRankTable(state, { rows, labelHead, columns }, pick));
  } else {
    // The lens brief above already names the list; a second heading only
    // pushes the first finding down.
    if (state.activeAnalysis === null) section.querySelector(":scope > h3")?.remove();
    section.appendChild(renderFindingList(rows, columns, pick));
  }
  panel.appendChild(section);
  if (analysisId === "health") {
    const summary = healthSummarySection(state);
    if (summary) panel.appendChild(summary);
  }
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

  const head = el("div", "panel-head is-text");
  const box = el("div", "file");
  const totalFiles = state.data.files.length;
  box.appendChild(
    el("div", "name", `${formatCount(matches.length)} file${matches.length === 1 ? "" : "s"}`),
  );
  box.appendChild(el("div", "dir", `Matches for "${query}"`));
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
      { rows: fileRankRows(state, matches), labelHead: "File", columns: usedByColumns },
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
        { rows: fileRankRows(state, affected), labelHead: "File", columns: usedByColumns },
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
  panelShell(
    panel,
    "Duplicated block",
    `${formatCount(group.lines)} lines in ${formatCount(group.instances.length)} places`,
    () => {
      state.selectedClone = null;
      refresh();
    },
  );
  panel.setAttribute("aria-label", "duplicated block");
  const fileCount = new Set(group.instances.map((instance) => instance.file)).size;
  const brief = el("section", "lens-brief");
  brief.appendChild(
    el(
      "p",
      "brief-purpose",
      fileCount === 1
        ? "The copies are in one file. Move the shared lines into one function."
        : `The copies are in ${formatCount(fileCount)} files. Move the shared code into one function or component that each file imports.`,
    ),
  );
  panel.appendChild(brief);

  const copies = sectionEl(`Every copy (${formatCount(group.instances.length)})`);
  const copiesTable = el("table", "rank-table");
  const copiesHead = el("thead");
  const copiesHr = el("tr");
  copiesHr.appendChild(el("th", "col-rank", "#"));
  copiesHr.appendChild(el("th", "col-file", "File"));
  copiesHr.appendChild(el("th", "col-val", "Lines"));
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
 * Open the panel with the shared head shell used by the drill-down panels:
 * the title opens the block and a meta line sits under it (the census has
 * no small label above a title). Returns the box for extra status lines.
 */
const panelShell = (
  panel: HTMLElement,
  title: string,
  meta: string,
  onClose: () => void,
): HTMLElement => {
  panel.replaceChildren();
  panel.classList.add("open");
  const head = el("div", "panel-head is-text");
  const box = el("div", "file");
  box.appendChild(el("div", "name", title));
  box.appendChild(el("div", "dir", meta));
  head.appendChild(box);
  head.appendChild(closeButton(onClose));
  panel.appendChild(head);
  return box;
};

type RoadRow = RankRow & { to: number };

/** Why one file pair of a road matters, and how badly. */
const roadPairNote = (forbidden: boolean, cyclic: boolean): { note: string; level: 0 | 1 | 2 } => {
  if (forbidden) return { note: ": forbidden by a boundary rule", level: 2 };
  if (cyclic) return { note: ": part of an import cycle", level: 1 };
  return { note: "", level: 0 };
};

/** One contributing file pair of a road as a finding row. */
const roadPairRow = (state: AppState, from: number, to: number): RoadRow => {
  const packed = from * state.data.files.length + to;
  const { note, level } = roadPairNote(
    state.index.violationEdges.has(packed),
    state.index.cycleEdges.has(packed),
  );
  const fromPath = state.data.files[from].path;
  return {
    label: basename(fromPath),
    dir: dirname(fromPath),
    metric: "",
    cells: [],
    fileIndex: from,
    to,
    why: `imports ${basename(state.data.files[to].path)}${note}`,
    level,
  };
};

/** The road headline: import count plus forbidden and cyclic parts. */
const roadBrief = (road: RoadSelection): HTMLElement => {
  const brief = el("section", "lens-brief");
  const head = el("div", "brief-head");
  head.appendChild(el("span", "brief-num", formatCount(road.count)));
  head.appendChild(el("span", "brief-unit", road.count === 1 ? "import" : "imports"));
  brief.appendChild(head);
  const parts: string[] = [];
  if (road.violations > 0) parts.push(`${formatCount(road.violations)} forbidden`);
  if (road.cycleEdges > 0) parts.push(`${formatCount(road.cycleEdges)} in an import cycle`);
  brief.appendChild(
    el(
      "p",
      "brief-purpose",
      parts.length > 0 ? parts.join(", ") : "No forbidden imports and no import cycles.",
    ),
  );
  return brief;
};

/** Which files of the target folder this traffic actually uses. */
const roadUsedSection = (
  state: AppState,
  road: RoadSelection,
  rows: RoadRow[],
  navigate: NavigateFn,
): HTMLElement => {
  const usage = new Map<number, number>();
  for (const row of rows) usage.set(row.to, (usage.get(row.to) ?? 0) + 1);
  const used = [...usage.entries()].toSorted((left, right) => right[1] - left[1]);
  const usedSection = sectionEl(`Files used from ${road.dstKey} (${formatCount(used.length)})`);
  const maxUse = used[0]?.[1] ?? 0;
  usedSection.appendChild(
    renderRankTable(
      state,
      {
        rows: used.map(([index, count]) => ({
          label: basename(state.data.files[index].path),
          dir: dirname(state.data.files[index].path),
          metric: `${formatCount(count)} importers`,
          cells: [{ value: formatCount(count), cls: "muted" }],
          fileIndex: index,
          bar: meterSpec(count, maxUse, "neutral"),
        })),
        labelHead: "File",
        columns: [{ header: "Importers", hint: `Files in ${road.srcKey} that import this file.` }],
      },
      (row) => {
        if (row.fileIndex !== null) navigate(row.fileIndex);
      },
    ),
  );
  return usedSection;
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
  panelShell(panel, `${road.srcKey} → ${road.dstKey}`, "Imports between folders", close);
  panel.setAttribute("aria-label", "imports between folders");
  const rows = road.pairs.map(([from, to]) => roadPairRow(state, from, to));
  panel.appendChild(roadBrief(road));
  panel.appendChild(roadUsedSection(state, road, rows, navigate));
  const section = sectionEl(`Every import (${formatCount(road.pairs.length)})`);
  section.appendChild(
    renderFindingList(
      rows.toSorted((left, right) => (right.level ?? 0) - (left.level ?? 0)),
      [],
      (row) => {
        if (row.fileIndex !== null) navigate(row.fileIndex);
      },
      ROAD_GROUPS,
    ),
  );
  panel.appendChild(section);
};
