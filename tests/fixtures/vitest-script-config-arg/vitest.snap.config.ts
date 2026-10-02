import { defineConfig } from "vitest/config";

export const snapshotDir = "snaps";

export default defineConfig({
  test: {
    include: ["snaps/**/*.snap.ts"],
  },
});
