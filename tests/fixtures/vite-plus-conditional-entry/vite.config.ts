import { defineConfig } from "vite-plus";

const packBootstrap = process.env.PACK_BOOTSTRAP === "1";
const packExecutable = process.env.PACK_EXECUTABLE === "1";

export default defineConfig({
  pack: {
    entry: packBootstrap
      ? ["src/bin.ts"]
      : packExecutable
        ? "src/binMain.ts"
        : ["src/bin.ts", "src/worker.ts"],
    outDir: packBootstrap ? "dist-exe" : "dist",
  },
});
