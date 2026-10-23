import path from "node:path";
import { defineConfig } from "vite";

export default defineConfig({
  resolve: { alias: { src: path.resolve(import.meta.dirname, "src") } },
});
