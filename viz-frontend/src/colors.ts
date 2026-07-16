import type { VizFileStatus } from "./types";

export interface ColorTheme {
  bg: string;
  text: string;
  textSecondary: string;
  border: string;
  hover: string;
  directory: string;
  directoryText: string;
  statusColors: Record<VizFileStatus, string>;
  tooltipBg: string;
  tooltipText: string;
  tooltipBorder: string;
  summaryBg: string;
  searchBg: string;
  breadcrumbBg: string;
  breadcrumbText: string;
}

const light: ColorTheme = {
  bg: "#ffffff",
  text: "#1a1a1a",
  textSecondary: "#666666",
  border: "rgba(0,0,0,0.15)",
  hover: "rgba(255,255,255,0.35)",
  directory: "#E8EDF2",
  directoryText: "#374151",
  statusColors: {
    clean: "#3B82F6",
    hasUnusedExports: "#F59E0B",
    unused: "#EF4444",
    entryPoint: "#10B981",
  },
  tooltipBg: "#ffffff",
  tooltipText: "#1a1a1a",
  tooltipBorder: "#e5e7eb",
  summaryBg: "#f9fafb",
  searchBg: "#ffffff",
  breadcrumbBg: "#f3f4f6",
  breadcrumbText: "#374151",
};

const dark: ColorTheme = {
  bg: "#1a1a2e",
  text: "#e0e0e0",
  textSecondary: "#999999",
  border: "rgba(255,255,255,0.1)",
  hover: "rgba(255,255,255,0.15)",
  directory: "#2A2D35",
  directoryText: "#d1d5db",
  statusColors: {
    clean: "#2563EB",
    hasUnusedExports: "#D97706",
    unused: "#DC2626",
    entryPoint: "#059669",
  },
  tooltipBg: "#1f2937",
  tooltipText: "#e0e0e0",
  tooltipBorder: "#374151",
  summaryBg: "#111827",
  searchBg: "#1f2937",
  breadcrumbBg: "#1f2937",
  breadcrumbText: "#d1d5db",
};

export const getTheme = (isDark: boolean): ColorTheme => (isDark ? dark : light);

export const detectDarkMode = (): boolean =>
  window.matchMedia("(prefers-color-scheme: dark)").matches;
