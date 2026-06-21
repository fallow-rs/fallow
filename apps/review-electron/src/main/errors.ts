/**
 * Translate raw `child_process` and Chromium `loadURL` failures into clean,
 * native-quality messages. Raw errors like `spawn fallow ENOENT` or
 * `ERR_CONNECTION_REFUSED (-102) loading 'http://...'` should never reach the UI.
 */

type ErrnoLike = NodeJS.ErrnoException & { stderr?: unknown };

/** First non-empty line of a (possibly multi-line) blob, length-capped. */
export const firstLine = (s: string): string => {
  const line =
    s
      .split("\n")
      .map((l) => l.trim())
      .find((l) => l.length > 0) ?? s.trim();
  return line.length > 200 ? `${line.slice(0, 199)}…` : line;
};

/** Clean message for a spawn/exec failure (missing binary, bad exit, etc.). */
export const describeExecError = (e: unknown, bin: string): Error => {
  const err = e as ErrnoLike;
  const name = bin.split("/").pop() || bin;
  if (err?.code === "ENOENT") {
    const hint =
      name === "fallow"
        ? " Set FALLOW_BIN or add fallow to your PATH."
        : ` Make sure "${name}" is installed and on your PATH.`;
    return new Error(`Couldn't find the "${name}" binary.${hint}`);
  }
  if (err?.code === "EACCES") return new Error(`Can't run "${name}": permission denied.`);
  const stderr = typeof err?.stderr === "string" ? err.stderr : "";
  const detail = stderr.trim() || (e instanceof Error ? e.message : String(e));
  return new Error(`"${name}" failed: ${firstLine(detail)}`);
};

/** Clean message for a Chromium `loadURL` failure when capturing a screenshot. */
export const describeLoadError = (e: unknown, url: string): Error => {
  const msg = e instanceof Error ? e.message : String(e);
  if (/ERR_CONNECTION_REFUSED/.test(msg)) {
    return new Error(`Couldn't reach ${url}. Is the dev server running?`);
  }
  if (/ERR_NAME_NOT_RESOLVED|ERR_INVALID_URL/.test(msg)) {
    return new Error(`Couldn't resolve ${url}. Check the URL.`);
  }
  const code = /(ERR_[A-Z_]+)/.exec(msg)?.[1];
  if (code)
    return new Error(`Couldn't load ${url} (${code.slice(4).toLowerCase().replace(/_/g, " ")}).`);
  return new Error(`Couldn't load ${url}: ${firstLine(msg)}`);
};
