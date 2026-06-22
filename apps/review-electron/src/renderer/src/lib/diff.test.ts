import { describe, it, expect } from "vitest";
import { parseUnifiedDiff, parseMultiFileDiff, diffStats } from "./diff";

const patch = `diff --git a/x.ts b/x.ts
index 1111111..2222222 100644
--- a/x.ts
+++ b/x.ts
@@ -1,3 +1,4 @@ fn header
 const a = 1;
-const b = 2;
+const b = 3;
+const c = 4;
 const d = 5;`;

describe("parseUnifiedDiff", () => {
  it("parses a hunk into typed rows with line numbers", () => {
    const hunks = parseUnifiedDiff(patch);
    expect(hunks).toHaveLength(1);
    expect(hunks[0]?.header).toBe("fn header");
    expect(hunks[0]?.range).toBe("-1,3 +1,4");
    const rows = hunks[0]?.rows ?? [];
    expect(rows[0]).toEqual({ kind: "context", oldNo: 1, newNo: 1, text: "const a = 1;" });
    expect(rows[1]).toEqual({ kind: "del", oldNo: 2, newNo: null, text: "const b = 2;" });
    expect(rows[2]).toEqual({ kind: "add", oldNo: null, newNo: 2, text: "const b = 3;" });
    expect(rows[3]).toEqual({ kind: "add", oldNo: null, newNo: 3, text: "const c = 4;" });
    expect(rows[4]).toEqual({ kind: "context", oldNo: 3, newNo: 4, text: "const d = 5;" });
  });

  it("ignores file headers and no-newline markers; empty -> []", () => {
    expect(parseUnifiedDiff("")).toEqual([]);
    expect(parseUnifiedDiff("diff --git a/x b/x\nindex 1..2\n--- a/x\n+++ b/x")).toEqual([]);
  });

  it("counts added/removed", () => {
    expect(diffStats(parseUnifiedDiff(patch))).toEqual({ added: 2, removed: 1 });
  });
});

describe("parseMultiFileDiff", () => {
  const multi = `diff --git a/x.ts b/x.ts
index 1..2 100644
--- a/x.ts
+++ b/x.ts
@@ -1,1 +1,2 @@
 const a = 1;
+const b = 2;
diff --git a/logo.png b/logo.png
index 3..4 100644
Binary files a/logo.png and b/logo.png differ
diff --git a/new.ts b/new.ts
new file mode 100644
--- /dev/null
+++ b/new.ts
@@ -0,0 +1 @@
+export const z = 1;`;

  it("splits a multi-file patch into per-file sections", () => {
    const sections = parseMultiFileDiff(multi);
    expect(sections.map((s) => s.file)).toEqual(["x.ts", "logo.png", "new.ts"]);
    expect(sections[0]?.binary).toBe(false);
    expect(sections[0]?.hunks[0]?.rows.at(-1)).toEqual({
      kind: "add",
      oldNo: null,
      newNo: 2,
      text: "const b = 2;",
    });
    // Binary files carry no hunks but are flagged.
    expect(sections[1]).toMatchObject({ file: "logo.png", binary: true });
    expect(sections[1]?.hunks).toEqual([]);
    // A new file uses the +++ side for its path.
    expect(sections[2]?.file).toBe("new.ts");
  });

  it("returns [] for an empty patch", () => {
    expect(parseMultiFileDiff("")).toEqual([]);
  });
});
