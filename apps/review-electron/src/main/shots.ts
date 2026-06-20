import { mkdir, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { appendFeedItem } from "./feed";

/** Decode a base64 `data:image/png` URL to raw bytes. */
export const decodePngDataUrl = (dataUrl: string): Buffer => {
  const match = /^data:image\/png;base64,(.+)$/s.exec(dataUrl);
  const b64 = match?.[1];
  if (b64 === undefined) throw new Error("expected a base64 png data url");
  return Buffer.from(b64, "base64");
};

export const shotPath = (root: string, at: number): string =>
  join(root, ".fallow-review", "shots", `shot-${at}.png`);

export type SaveAnnotation = {
  annotatedDataUrl: string;
  note: string;
  target?: string;
};

/** Persist an annotated screenshot and append a feed item referencing it. */
export const saveAnnotatedShot = async (
  root: string,
  payload: SaveAnnotation,
  at: number,
): Promise<string> => {
  const png = decodePngDataUrl(payload.annotatedDataUrl);
  const path = shotPath(root, at);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, png);
  await appendFeedItem(root, {
    target: { kind: "file_line", value: payload.target ?? "screenshot" },
    note: payload.note,
    imageRef: path,
    at: new Date(at).toISOString(),
  });
  return path;
};
