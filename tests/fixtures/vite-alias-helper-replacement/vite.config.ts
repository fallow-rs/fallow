import { defineConfig } from "vite";
import { fileURLToPath, URL } from "node:url";

const here = (relative: string) => fileURLToPath(new URL(relative, import.meta.url));

function fromRoot(relative: string) {
  return fileURLToPath(new URL(relative, import.meta.url));
}

export default defineConfig({
  resolve: {
    alias: [
      { find: "local-widget", replacement: here("src/widget.ts") },
      { find: "local-format", replacement: fromRoot("./src/format.ts") },
    ],
  },
});
