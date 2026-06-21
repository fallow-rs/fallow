import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

export type AppConfig = {
  fallowBin: string | null;
  defaultUrl: string;
  inspectPort: number;
  agentBackend: string;
  diffBase: string | null;
};

export const DEFAULT_CONFIG: AppConfig = {
  fallowBin: null,
  defaultUrl: "http://localhost:5273",
  inspectPort: 7787,
  agentBackend: "claude-code",
  diffBase: null,
};

/**
 * JSONC -> JSON: strip block comments, full-line `//` comments, and trailing
 * commas. Full-line only, so `http://` inside string values is preserved.
 */
export const stripJsonc = (text: string): string =>
  text
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/^\s*\/\/.*$/gm, "")
    .replace(/,(\s*[}\]])/g, "$1");

const str = (v: unknown, fallback: string): string => (typeof v === "string" ? v : fallback);
const strOrNull = (v: unknown, fallback: string | null): string | null =>
  typeof v === "string" ? v : fallback;
const num = (v: unknown, fallback: number): number =>
  typeof v === "number" && Number.isFinite(v) ? v : fallback;

/** Parse a JSONC config string, merging over defaults; invalid input -> defaults. */
export const parseConfig = (text: string): AppConfig => {
  let raw: unknown;
  try {
    raw = JSON.parse(stripJsonc(text));
  } catch {
    return { ...DEFAULT_CONFIG };
  }
  if (typeof raw !== "object" || raw === null) return { ...DEFAULT_CONFIG };
  const r = raw as Record<string, unknown>;
  return {
    fallowBin: strOrNull(r["fallowBin"], DEFAULT_CONFIG.fallowBin),
    defaultUrl: str(r["defaultUrl"], DEFAULT_CONFIG.defaultUrl),
    inspectPort: num(r["inspectPort"], DEFAULT_CONFIG.inspectPort),
    agentBackend: str(r["agentBackend"], DEFAULT_CONFIG.agentBackend),
    diffBase: strOrNull(r["diffBase"], DEFAULT_CONFIG.diffBase),
  };
};

export const configPath = (home: string = homedir()): string =>
  join(home, ".fallow-review", "config.jsonc");

/** Load the user config (or defaults if missing/unreadable). */
export const loadConfig = (home: string = homedir()): AppConfig => {
  try {
    return parseConfig(readFileSync(configPath(home), "utf8"));
  } catch {
    return { ...DEFAULT_CONFIG };
  }
};
