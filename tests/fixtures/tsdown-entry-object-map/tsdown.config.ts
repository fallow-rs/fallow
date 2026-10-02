import { defineConfig } from "tsdown";

export default defineConfig({
  entry: { main: "src/main.ts", worker: "src/worker.ts" },
});
