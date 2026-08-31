import type { AppState } from "./state";
import type { Lens, SecondaryAnalysis } from "./types";

export type LensSeverity = "error" | "warn" | "neutral";
export type LensAvailability = "complete" | "disabled" | "notApplicable" | "unavailable";

export interface LensCount {
  value: number;
  unit: string;
}

export interface LensAvailabilityDetails {
  state: LensAvailability;
  reason: string | null;
  truncated: number;
}

export interface LensDefinition {
  id: Lens;
  name: string;
  gloss: string;
  shortcut: string;
  severity: LensSeverity;
  count: (state: AppState) => LensCount | null;
}

export interface SecondaryAnalysisDefinition {
  id: SecondaryAnalysis;
  name: string;
  gloss: string;
}

export interface SecondaryAnalysisMenuItem extends SecondaryAnalysisDefinition {
  active: boolean;
  availability: LensAvailabilityDetails & LensCount;
}

const numberProperty = (value: unknown, property: string): number | null => {
  if (typeof value !== "object" || value === null) return null;
  const candidate = Reflect.get(value, property);
  return typeof candidate === "number" && Number.isFinite(candidate) ? candidate : null;
};

const stringProperty = (value: unknown, property: string): string | null => {
  if (typeof value !== "object" || value === null) return null;
  const candidate = Reflect.get(value, property);
  return typeof candidate === "string" ? candidate : null;
};

const arrayLength = (value: unknown, property: string): number | null => {
  if (typeof value !== "object" || value === null) return null;
  const candidate = Reflect.get(value, property);
  return Array.isArray(candidate) ? candidate.length : null;
};

const summaryCount = (state: AppState, property: string): number | null =>
  numberProperty(state.data.summary, property);

const analysisSection = (state: AppState, property: string): unknown =>
  Reflect.get(state.data, property);

const analysisAvailabilityValue = (state: AppState, property: string): unknown => {
  const section = analysisSection(state, property);
  return typeof section === "object" && section !== null
    ? Reflect.get(section, "availability")
    : null;
};

const analysisCount = (state: AppState, property: string): number | null =>
  numberProperty(analysisAvailabilityValue(state, property), "count");

const analysisMetric = (
  state: AppState,
  property: string,
  fallbackUnit: string,
): LensCount | null => {
  const availability = analysisAvailabilityValue(state, property);
  const value = numberProperty(availability, "count");
  if (value === null) return null;
  return { value, unit: stringProperty(availability, "unit") ?? fallbackUnit };
};

const securityCount = (state: AppState): number | null => {
  const fromAvailability = analysisCount(state, "security");
  if (fromAvailability !== null) return fromAvailability;
  const fromSummary =
    summaryCount(state, "security_candidates") ?? summaryCount(state, "security_findings");
  if (fromSummary !== null) return fromSummary;
  const security = Reflect.get(state.data, "security");
  return arrayLength(security, "candidates") ?? arrayLength(security, "findings");
};

const count = (value: number | null, unit: string): LensCount | null =>
  value === null ? null : { value, unit };

export const LENSES: readonly LensDefinition[] = [
  {
    id: "overview",
    name: "Overview",
    gloss: "Folders and imports",
    shortcut: "1",
    severity: "neutral",
    count: () => null,
  },
  {
    id: "unused",
    name: "Unused",
    gloss: "Dead files and exports",
    shortcut: "2",
    severity: "error",
    count: (state) =>
      count(
        state.data.files.filter((file) => file.status === "unused" || file.unused_export_count > 0)
          .length,
        "affected files",
      ),
  },
  {
    id: "duplication",
    name: "Duplication",
    gloss: "Copy-pasted code",
    shortcut: "3",
    severity: "warn",
    count: (state) => count(summaryCount(state, "clone_groups"), "clone groups"),
  },
  {
    id: "architecture",
    name: "Architecture",
    gloss: "Boundaries and dependency cycles",
    shortcut: "4",
    severity: "error",
    count: (state) =>
      analysisMetric(state, "architecture", "violations") ??
      count(summaryCount(state, "boundary_violations"), "violations"),
  },
  {
    id: "health",
    name: "Health",
    gloss: "Maintainability and change risk",
    shortcut: "5",
    severity: "warn",
    count: (state) =>
      analysisMetric(state, "health", "hotspot files") ??
      count(summaryCount(state, "hotspot_files"), "hotspot files"),
  },
  {
    id: "security",
    name: "Security",
    gloss: "Static security candidates and blind spots",
    shortcut: "6",
    severity: "error",
    count: (state) =>
      analysisMetric(state, "security", "candidates") ?? count(securityCount(state), "candidates"),
  },
] as const;

export const LENS_IDS: readonly Lens[] = LENSES.map((lens) => lens.id);

export const SECONDARY_ANALYSES: readonly SecondaryAnalysisDefinition[] = [
  {
    id: "dependencies",
    name: "Dependencies",
    gloss: "Dependency hygiene and public API findings",
  },
  {
    id: "frameworks",
    name: "Frameworks",
    gloss: "Framework-specific analysis",
  },
  {
    id: "styling",
    name: "Styling",
    gloss: "Stylesheet and design-system analysis",
  },
  {
    id: "feature_flags",
    name: "Feature flags",
    gloss: "Feature flag definitions and usage",
  },
] as const;

const SECONDARY_BY_ID = new Map<SecondaryAnalysis, SecondaryAnalysisDefinition>(
  SECONDARY_ANALYSES.map((analysis) => [analysis.id, analysis]),
);

export const secondaryAnalysisById = (analysis: SecondaryAnalysis): SecondaryAnalysisDefinition => {
  const definition = SECONDARY_BY_ID.get(analysis);
  if (!definition) throw new Error(`Unknown secondary analysis: ${analysis}`);
  return definition;
};

export const parseSecondaryAnalysis = (value: string | null): SecondaryAnalysis | null => {
  if (value === "flags") return "feature_flags";
  return SECONDARY_ANALYSES.find((analysis) => analysis.id === value)?.id ?? null;
};

export interface LensWidth {
  id: Lens;
  width: number;
}

/**
 * Pick the primary lens buttons that fit next to the More control. The active
 * lens is always retained, even when it is later in the canonical order.
 */
export const fitLensNavigation = (
  widths: readonly LensWidth[],
  availableWidth: number,
  moreWidth: number,
  activeLens: Lens,
  gap = 4,
): Lens[] => {
  const totalWidth = widths.reduce(
    (total, item, index) => total + item.width + (index === 0 ? 0 : gap),
    0,
  );
  if (totalWidth <= availableWidth) return widths.map((item) => item.id);

  const active = widths.find((item) => item.id === activeLens);
  const room = Math.max(0, availableWidth - moreWidth - gap);
  const visible: LensWidth[] = [];
  let used = 0;
  for (const item of widths) {
    const next = item.width + (visible.length === 0 ? 0 : gap);
    if (used + next > room) break;
    visible.push(item);
    used += next;
  }

  if (active && !visible.some((item) => item.id === active.id)) {
    while (visible.length > 0) {
      const candidateWidth =
        visible.reduce((total, item, index) => total + item.width + (index === 0 ? 0 : gap), 0) +
        gap +
        active.width;
      if (candidateWidth <= room) break;
      visible.pop();
    }
    visible.push(active);
  }

  return widths
    .filter((item) => visible.some((candidate) => candidate.id === item.id))
    .map((item) => item.id);
};

const LENS_BY_ID = new Map<Lens, LensDefinition>(LENSES.map((lens) => [lens.id, lens]));

const LEGACY_LENS_ALIASES: Readonly<Record<string, Lens>> = {
  deadcode: "unused",
  dupes: "duplication",
  boundaries: "architecture",
  hotspots: "health",
};

export const lensById = (lens: Lens): LensDefinition => {
  const definition = LENS_BY_ID.get(lens);
  if (!definition) throw new Error(`Unknown visualization lens: ${lens}`);
  return definition;
};

export const parseLens = (value: string | null): Lens | null => {
  if (value === null) return null;
  const alias = LEGACY_LENS_ALIASES[value];
  if (alias) return alias;
  return LENSES.find((lens) => lens.id === value)?.id ?? null;
};

export const lensAvailabilityDetails = (state: AppState, lens: Lens): LensAvailabilityDetails => {
  const sectionName =
    lens === "architecture" || lens === "health" || lens === "security" ? lens : null;
  if (sectionName === null) return { state: "complete", reason: null, truncated: 0 };

  const availability = analysisAvailabilityValue(state, sectionName);
  if (typeof availability !== "object" || availability === null) {
    const legacyCount = lensById(lens).count(state);
    return {
      state: legacyCount === null ? "unavailable" : "complete",
      reason: legacyCount === null ? "This report does not contain this analysis" : null,
      truncated: 0,
    };
  }
  const availabilityState = Reflect.get(availability, "state");
  if (
    availabilityState === "complete" ||
    availabilityState === "disabled" ||
    availabilityState === "notApplicable" ||
    availabilityState === "unavailable"
  ) {
    const reason = Reflect.get(availability, "reason");
    return {
      state: availabilityState,
      reason: typeof reason === "string" ? reason : null,
      truncated: numberProperty(availability, "truncated") ?? 0,
    };
  }

  return { state: "unavailable", reason: "Invalid analysis availability", truncated: 0 };
};

export const lensAvailability = (state: AppState, lens: Lens): LensAvailability =>
  lensAvailabilityDetails(state, lens).state;

export const secondaryAvailabilityDetails = (
  state: AppState,
  analysis: SecondaryAnalysis,
): LensAvailabilityDetails & LensCount => {
  const availability = analysisAvailabilityValue(state, analysis);
  if (typeof availability !== "object" || availability === null) {
    return {
      state: "unavailable",
      value: 0,
      unit: "findings",
      reason: "This report does not contain this analysis",
      truncated: 0,
    };
  }
  const availabilityState = Reflect.get(availability, "state");
  const stateValue: LensAvailability =
    availabilityState === "complete" ||
    availabilityState === "disabled" ||
    availabilityState === "notApplicable" ||
    availabilityState === "unavailable"
      ? availabilityState
      : "unavailable";
  const value = numberProperty(availability, "count") ?? 0;
  return {
    state: stateValue,
    value,
    unit: stringProperty(availability, "unit") ?? "findings",
    reason: stringProperty(availability, "reason"),
    truncated: numberProperty(availability, "truncated") ?? 0,
  };
};

export const secondaryAnalysisMenuItems = (state: AppState): SecondaryAnalysisMenuItem[] =>
  SECONDARY_ANALYSES.map((definition) => ({
    ...definition,
    active: state.activeAnalysis === definition.id,
    availability: secondaryAvailabilityDetails(state, definition.id),
  }));
