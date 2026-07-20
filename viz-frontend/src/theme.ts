/**
 * Fallow design tokens for the viz canvas layers.
 *
 * Mirrors the terminal-brutalist design system: Radix Sand Dark neutrals,
 * strict semantic accents (red = error, amber = warn, green = pass,
 * blue = info/interactive), and a CVD-validated categorical palette for
 * boundary zones. DOM chrome reads the same values from CSS custom
 * properties in styles.css; this module feeds the canvas renderers.
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

/** Sand Dark based theme (Mode A dashboard). */
const dark: Theme = {
  bg: "#111110",
  surface1: "#191918",
  surface2: "#222221",
  surface3: "#2a2a28",
  textHigh: "#eeeeec",
  textLow: "#b5b3ad",
  textMuted: "#6f6d66",
  borderSubtle: "#3b3a37",
  borderDefault: "#494844",
  borderStrong: "#62605b",
  red: "#e5484d",
  redText: "#ff9592",
  redSubtle: "#3b1219",
  amber: "#ffc53d",
  amberText: "#ffca16",
  amberSubtle: "#302008",
  green: "#30a46c",
  greenText: "#3dd68c",
  blue: "#0090ff",
  blueText: "#70b8ff",
  blueSubtle: "#0d2847",
  dirFill: "#191918",
  dirHeader: "#222221",
  cellNeutral: "#45443f",
  cellEntry: "#2b3f56",
  zones: ["#0090ff", "#e35b00", "#12a594", "#5b5bd6", "#d6409f", "#4c9a2a", "#a855c9", "#64748b"],
  zoneOther: "#62605b",
};

/** Light (Mode B report) override. */
const light: Theme = {
  bg: "#ffffff",
  surface1: "#f9f9f8",
  surface2: "#f2f2f0",
  surface3: "#ebebea",
  textHigh: "#21201c",
  textLow: "#6f6d66",
  textMuted: "#908e86",
  borderSubtle: "#e3e2de",
  borderDefault: "#cfceca",
  borderStrong: "#a9a7a0",
  red: "#ce2c31",
  redText: "#ce2c31",
  redSubtle: "#fff1f0",
  amber: "#e2a336",
  amberText: "#ad5700",
  amberSubtle: "#fefbe9",
  green: "#30a46c",
  greenText: "#18794e",
  blue: "#0090ff",
  blueText: "#0d74ce",
  blueSubtle: "#edf6ff",
  dirFill: "#f9f9f8",
  dirHeader: "#f2f2f0",
  cellNeutral: "#a9a7a0",
  cellEntry: "#7fb1e8",
  zones: ["#0090ff", "#e35b00", "#12a594", "#5b5bd6", "#d6409f", "#4c9a2a", "#a855c9", "#64748b"],
  zoneOther: "#a9a7a0",
};

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
  return mix(mix(theme.amberSubtle, theme.amber, 0.25), theme.amber, Math.min(1, intensity));
};

/**
 * Two-stop warm ramp for the hotspot lens: neutral → amber → red
 * (matches the design system's severity gradient). `intensity` in [0, 1].
 */
export const heatRamp = (theme: Theme, intensity: number): string => {
  if (intensity <= 0) return theme.cellNeutral;
  const clamped = Math.min(1, intensity);
  if (clamped < 0.5) {
    return mix(mix(theme.amberSubtle, theme.amber, 0.3), theme.amber, clamped * 2);
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
  return luminance > 0.55 ? "#111110" : "#eeeeec";
};
