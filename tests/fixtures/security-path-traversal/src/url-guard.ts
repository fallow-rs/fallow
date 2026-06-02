import * as path from "node:path";

const ALLOWED_URLS = ["https://example.com/avatar.png"];

export function resolveUpload(userPath: string): string {
  if (!ALLOWED_URLS.includes(userPath)) {
    throw new Error("url not allowed");
  }

  return path.join(userPath, "avatar.png");
}
