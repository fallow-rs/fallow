import { defineConfig } from "tsup";

const shared = { format: ["esm"] };

export default defineConfig([
  { entry: ["src/main.ts"] },
  { ...shared, entry: { worker: "src/worker.ts" } },
]);
