// Negative: literal sink arguments are not captured, and source-free mass assignment is gated out.
import * as fs from "node:fs";

export function literalRequire(): unknown {
  return require("known-plugin");
}

export function literalFsRead(): string {
  return fs.readFileSync("safe.txt", "utf8");
}

export function literalHeader(res: { setHeader(name: string, value: string): void }): void {
  res.setHeader("X-Static", "ok");
}

export const literalWriteHead = (res: {
  writeHead(status: number, headers: Record<string, string | string[]>): void;
}): void => {
  res.writeHead(200, {
    "Content-Type": "text/html",
    "Set-Cookie": ["session=static"],
  });
};

export const literalWriteHeadWithStatusMessage = (res: {
  writeHead(
    status: number,
    statusMessage: string,
    headers: Record<string, string>,
  ): void;
}): void => {
  res.writeHead(200, "OK", { "Content-Type": "text/html" });
};

export function sourceFreeAssign(target: Record<string, unknown>, defaults: Record<string, unknown>): void {
  Object.assign(target, defaults);
}
