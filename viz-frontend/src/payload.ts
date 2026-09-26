import type { VizData } from "./types";

/**
 * Payload transport between `fallow viz` and this bundle.
 *
 * The CLI writes the core payload into `<script id="fallow-data">` and each
 * large lens section into its own `<script data-fallow-lazy="path">` tag.
 * Both use `type="application/json"`, so the browser does not parse them as
 * script. The core is parsed at start. A lazy section is parsed on the first
 * read of its property, which is normally when its lens opens.
 *
 * Arrays of objects can arrive as tables: `{"$k": keys, "$r": rows}`. A
 * `null` cell or a missing trailing cell means that the key is absent.
 * The core tag has a `data-tables` attribute only when tables are in use.
 * The Rust side (`crates/cli/src/viz/payload.rs`) owns the encoder.
 */

/** One deferred payload section. `text` is called at most once. */
export interface LazySection {
  /** `a.b` puts the value at `data.a.b`; `a.*.b` spreads one entry per element of `data.a`. */
  path: string;
  text: () => string;
}

const TABLE_KEYS = "$k";
const TABLE_ROWS = "$r";
const COLUMN_MARKER = "*";

type JsonObject = Record<string, unknown>;

const isObject = (value: unknown): value is JsonObject =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const isTable = (value: JsonObject): boolean => {
  const keys = Object.keys(value);
  return keys.length === 2 && Array.isArray(value[TABLE_KEYS]) && Array.isArray(value[TABLE_ROWS]);
};

const decodeRow = (keys: string[], row: unknown[]): JsonObject => {
  const out: JsonObject = {};
  for (let column = 0; column < keys.length && column < row.length; column++) {
    const cell = row[column];
    if (cell !== null) out[keys[column]] = decodeTables(cell);
  }
  return out;
};

/** Expand every `{"$k", "$r"}` table in a parsed JSON value into objects. */
export const decodeTables = (value: unknown): unknown => {
  if (Array.isArray(value)) return value.map(decodeTables);
  if (!isObject(value)) return value;
  if (isTable(value)) {
    const keys = value[TABLE_KEYS] as string[];
    return (value[TABLE_ROWS] as unknown[][]).map((row) => decodeRow(keys, row));
  }
  const out: JsonObject = {};
  for (const [key, entry] of Object.entries(value)) out[key] = decodeTables(entry);
  return out;
};

interface ParsedSection {
  path: string;
  parse: () => unknown;
}

const setValue = (target: JsonObject, key: string, value: unknown): void => {
  Object.defineProperty(target, key, {
    value,
    writable: true,
    enumerable: true,
    configurable: true,
  });
};

const defineLazy = (target: JsonObject, key: string, load: () => void): void => {
  Object.defineProperty(target, key, {
    configurable: true,
    enumerable: true,
    get: () => {
      load();
      return target[key];
    },
    set: (value: unknown) => setValue(target, key, value),
  });
};

const resolveParent = (root: JsonObject, segments: string[]): unknown => {
  let node: unknown = root;
  for (const segment of segments) {
    if (!isObject(node)) return undefined;
    node = node[segment];
  }
  return node;
};

const installProperty = (root: JsonObject, section: ParsedSection, segments: string[]): void => {
  const key = segments[segments.length - 1];
  const parent = resolveParent(root, segments.slice(0, -1));
  if (!isObject(parent)) return;
  defineLazy(parent, key, () => setValue(parent, key, section.parse()));
};

const installColumn = (root: JsonObject, section: ParsedSection, segments: string[]): void => {
  const marker = segments.indexOf(COLUMN_MARKER);
  const key = segments[marker + 1];
  const rows = resolveParent(root, segments.slice(0, marker));
  if (!Array.isArray(rows) || key === undefined) return;
  const elements = rows.filter(isObject);
  let loaded = false;
  const load = (): void => {
    if (loaded) return;
    loaded = true;
    let values: unknown[] = [];
    try {
      const column = section.parse();
      if (Array.isArray(column)) values = column;
    } finally {
      // Replace every getter, also when the parse fails, so a later read
      // cannot enter the getter again.
      elements.forEach((element, index) => {
        const value = values[index];
        if (Array.isArray(value) && value.length > 0) setValue(element, key, value);
        else delete element[key];
      });
    }
  };
  for (const element of elements) defineLazy(element, key, load);
};

/** Parse the core payload and attach each lazy section at its path. */
export const hydratePayload = (
  coreText: string,
  sections: LazySection[],
  tables = true,
): VizData => {
  const decode = (text: string): unknown =>
    tables ? decodeTables(JSON.parse(text)) : JSON.parse(text);
  const root = decode(coreText);
  if (!isObject(root)) throw new Error("fallow viz payload is not an object");
  for (const raw of sections) {
    const section: ParsedSection = { path: raw.path, parse: () => decode(raw.text()) };
    const segments = section.path.split(".");
    if (segments.includes(COLUMN_MARKER)) installColumn(root, section, segments);
    else installProperty(root, section, segments);
  }
  return root as unknown as VizData;
};

/** Read the payload tags that `fallow viz` writes into the page. */
export const readEmbeddedPayload = (doc: Document): VizData | null => {
  const core = doc.getElementById("fallow-data");
  const coreText = core?.textContent;
  if (!core || !coreText) return null;
  core.remove();
  const sections = [...doc.querySelectorAll<HTMLScriptElement>("script[data-fallow-lazy]")].map(
    (element): LazySection => ({
      path: element.dataset.fallowLazy ?? "",
      text: () => {
        // Drop the tag once it is read, so the page does not keep a
        // second copy of the section text in the DOM.
        const text = element.textContent ?? "null";
        element.remove();
        return text;
      },
    }),
  );
  return hydratePayload(coreText, sections, core.hasAttribute("data-tables"));
};
