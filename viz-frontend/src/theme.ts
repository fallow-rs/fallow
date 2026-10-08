/**
 * Census design tokens for the viz canvas layers.
 *
 * Mirrors "The Traffic Census" of fallow.tools and fallow.cloud: paper and
 * ink, hairline rules, and four enamel route colors that carry severity
 * (billing red = high, coverage ochre = medium, ingest green = pass, auth
 * blue = info and focus). The surface is flat, so tiles get no relief. DOM
 * chrome reads the same values from CSS custom properties in styles.css;
 * this module feeds the canvas renderers.
 */

export interface Theme {
  /** Page background (surface-0). */
  bg: string;
  /** Raised surface (cards, panel). */
  surface1: string;
  /** Floating surface (tooltips). */
  surface2: string;
  /** Overlay surface. */
  surface3: string;
  /** Primary text. */
  textHigh: string;
  /** Secondary text. */
  textLow: string;
  /** De-emphasized text. */
  textMuted: string;
  /** Subtle borders. */
  borderSubtle: string;
  /** Default borders. */
  borderDefault: string;
  /** Strong borders / focus emphasis. */
  borderStrong: string;
  /** Error severity. */
  red: string;
  redText: string;
  redSubtle: string;
  /** Warn severity. */
  amber: string;
  amberText: string;
  amberSubtle: string;
  /** Pass. */
  green: string;
  greenText: string;
  /** Info / interactive. */
  blue: string;
  blueText: string;
  blueSubtle: string;
  /** Treemap directory fill (between surface and cells). */
  dirFill: string;
  dirHeader: string;
  /** Neutral cell fill for "clean" files. */
  cellNeutral: string;
  /** Recessive blue-tinted fill for entry points (info, not a finding). */
  cellEntry: string;
  /** Categorical zone palette (CVD-validated, fixed order, never cycled). */
  zones: string[];
  /** Fold color for zones beyond the palette and files without a zone. */
  zoneOther: string;
}

/** Night census: the same map after dark. */
const dark: Theme = {
  bg: "#121211",
  surface1: "#121211",
  surface2: "#1b1b19",
  surface3: "#242422",
  textHigh: "#edece7",
  textLow: "#c9c8c1",
  textMuted: "#a3a29a",
  borderSubtle: "#34332f",
  borderDefault: "#6f6e68",
  borderStrong: "#edece7",
  red: "#ef5f4b",
  redText: "#ff7a66",
  redSubtle: "#2e1714",
  amber: "#dca42a",
  amberText: "#e2ad3a",
  amberSubtle: "#2a2210",
  green: "#2ebd85",
  greenText: "#3cc792",
  blue: "#4d9df5",
  blueText: "#6aaeff",
  blueSubtle: "#132235",
  dirFill: "#121211",
  dirHeader: "#1b1b19",
  cellNeutral: "#2e2d2a",
  cellEntry: "#1f3a5a",
  zones: ["#4d9df5", "#e8833a", "#2ebd85", "#8b8bf0", "#e06aae", "#7dbb4f", "#b77ad6", "#8a93a3"],
  zoneOther: "#6f6e68",
};

/** Day census: timetable paper and ink, the default look. */
const light: Theme = {
  bg: "#f7f7f4",
  surface1: "#f7f7f4",
  surface2: "#efeee9",
  surface3: "#e6e5df",
  textHigh: "#1a1a1a",
  textLow: "#3d3c39",
  textMuted: "#5c5b56",
  borderSubtle: "#d6d5cf",
  borderDefault: "#85847e",
  borderStrong: "#1a1a1a",
  red: "#d23a2a",
  redText: "#b32e20",
  redSubtle: "#f5e3df",
  amber: "#a87300",
  amberText: "#8a5e00",
  amberSubtle: "#f3ead6",
  green: "#00875a",
  greenText: "#006b47",
  blue: "#0066c0",
  blueText: "#0066c0",
  blueSubtle: "#e1ebf5",
  dirFill: "#f7f7f4",
  dirHeader: "#efeee9",
  cellNeutral: "#dcdbd5",
  cellEntry: "#b9d0e8",
  zones: ["#0066c0", "#c25400", "#00875a", "#5b5bd6", "#c2307f", "#4c8a1f", "#8f45b3", "#5f6b7a"],
  zoneOther: "#85847e",
};

/** Canvas font stacks: the census text face and its condensed display cut. */
export const TEXT_FACE = '"Barlow", "Barlow Fallback", system-ui, sans-serif';
export const DISPLAY_FACE =
  '"Barlow Semi Condensed", "Barlow Semi Condensed Fallback", "Barlow", system-ui, sans-serif';

export const getTheme = (isDark: boolean): Theme => (isDark ? dark : light);

export const prefersReducedMotion = (): boolean =>
  typeof window.matchMedia === "function" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// ── Color math for lens ramps ───────────────────────────────────

interface Rgb {
  r: number;
  g: number;
  b: number;
}

const hexToRgb = (hex: string): Rgb => ({
  r: parseInt(hex.slice(1, 3), 16),
  g: parseInt(hex.slice(3, 5), 16),
  b: parseInt(hex.slice(5, 7), 16),
});

const rgbToHex = ({ r: red, g: green, b: blue }: Rgb): string =>
  `#${[red, green, blue].map((channel) => Math.round(channel).toString(16).padStart(2, "0")).join("")}`;

export const mix = (fromColor: string, toColor: string, ratio: number): string => {
  const fromRgb = hexToRgb(fromColor);
  const toRgb = hexToRgb(toColor);
  return rgbToHex({
    r: fromRgb.r + (toRgb.r - fromRgb.r) * ratio,
    g: fromRgb.g + (toRgb.g - fromRgb.g) * ratio,
    b: fromRgb.b + (toRgb.b - fromRgb.b) * ratio,
  });
};

/**
 * Sequential single-hue ramp for the duplication lens: neutral → amber.
 * `intensity` in [0, 1].
 */
export const dupRamp = (theme: Theme, intensity: number): string => {
  if (intensity <= 0) return theme.cellNeutral;
  // Starts from a muted ochre near the neutral fill, so a sliver of copied
  // code stays calm and only heavy duplication reaches full amber.
  const floor = mix(theme.cellNeutral, theme.amber, 0.38);
  return mix(floor, theme.amber, Math.min(1, intensity) ** 0.8);
};

/**
 * Two-stop warm ramp for the hotspot lens: neutral → amber → red
 * (matches the design system's severity gradient). `intensity` in [0, 1].
 */
export const heatRamp = (theme: Theme, intensity: number): string => {
  if (intensity <= 0) return theme.cellNeutral;
  const clamped = Math.min(1, intensity);
  // The low end leans on the neutral fill: most flagged files carry a
  // small risk, and a full-amber floor drowned the few that matter.
  if (clamped < 0.5) {
    const floor = mix(theme.cellNeutral, theme.amber, 0.38);
    return mix(floor, theme.amber, (clamped * 2) ** 1.4);
  }
  return mix(theme.amber, theme.red, (clamped - 0.5) * 2);
};

/** Zone color by index, folding overflow into the neutral "other" slot. */
export const zoneColor = (theme: Theme, zone: number | undefined): string => {
  if (zone === undefined) return theme.cellNeutral;
  return zone < theme.zones.length ? theme.zones[zone] : theme.zoneOther;
};

/** Text color that contrasts with an arbitrary hex fill. */
export const contrastText = (hex: string): string => {
  const { r: red, g: green, b: blue } = hexToRgb(hex);
  const luminance = (0.299 * red + 0.587 * green + 0.114 * blue) / 255;
  return luminance > 0.55 ? "#1a1a1a" : "#f7f7f4";
};
