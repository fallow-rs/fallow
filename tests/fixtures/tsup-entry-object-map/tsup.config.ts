import { defineConfig } from "tsup";

export default defineConfig({
  entry: { main: "src/main.ts", worker: "src/worker.ts" },
});
