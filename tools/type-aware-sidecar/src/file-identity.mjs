import { realpathSync } from "node:fs";
import path from "node:path";

export const canonicalFileIdentity = (fileName) => {
  const absolutePath = path.normalize(path.resolve(fileName));
  try {
    return path.normalize(realpathSync.native(absolutePath));
  } catch {
    return absolutePath;
  }
};
