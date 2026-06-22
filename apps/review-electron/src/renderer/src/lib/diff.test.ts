import { describe, it, expect } from "vitest";
import { parseUnifiedDiff, diffStats } from "./diff";

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
