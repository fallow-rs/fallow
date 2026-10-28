import { defineConfig } from "vite";
import { fileURLToPath, URL } from "node:url";

const p = (relative: string) => fileURLToPath(new URL(relative, import.meta.url));

export default defineConfig({
  resolve: {
    alias: [
      { find: /^node:module$/, replacement: p("./src/node-module-shim.ts") },
      { find: /^virtual-logger$/, replacement: p("./src/logger.ts") },
      { find: /^@\/(.*)$/, replacement: p("./src/$1") },
    ],
  },
});
