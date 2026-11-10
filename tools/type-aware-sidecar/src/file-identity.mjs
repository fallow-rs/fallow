import { realpathSync } from "node:fs";
import path from "node:path";

// One request resolves the same file many times, and each resolution is a
// system call. The cache lives for one request, so a symlink that changes
// between session requests resolves again.
const identities = new Map();

/** Forgets the resolved file identities. Call it at the start of each request. */
export const resetFileIdentityCache = () => identities.clear();

const resolveIdentity = (absolutePath) => {
  try {
    return path.normalize(realpathSync.native(absolutePath));
  } catch {
    return absolutePath;
  }
};

export const canonicalFileIdentity = (fileName) => {
  const absolutePath = path.normalize(path.resolve(fileName));
  const cached = identities.get(absolutePath);
  if (cached !== undefined) return cached;
  const identity = resolveIdentity(absolutePath);
  identities.set(absolutePath, identity);
  return identity;
};
