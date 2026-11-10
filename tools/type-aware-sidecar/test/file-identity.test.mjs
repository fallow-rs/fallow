import assert from "node:assert/strict";
import { mkdtempSync, realpathSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";

import { canonicalFileIdentity, resetFileIdentityCache } from "../src/file-identity.mjs";

test(
  "a reset makes a changed symlink resolve again",
  { skip: process.platform === "win32" },
  () => {
    const root = realpathSync(mkdtempSync(path.join(tmpdir(), "fallow-file-identity-")));
    try {
      const first = path.join(root, "first.ts");
      const second = path.join(root, "second.ts");
      const link = path.join(root, "link.ts");
      writeFileSync(first, "");
      writeFileSync(second, "");
      symlinkSync(first, link);
      resetFileIdentityCache();
      assert.equal(canonicalFileIdentity(link), first);

      unlinkSync(link);
      symlinkSync(second, link);
      assert.equal(canonicalFileIdentity(link), first, "one request keeps its resolution");
      resetFileIdentityCache();
      assert.equal(canonicalFileIdentity(link), second);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  },
);
