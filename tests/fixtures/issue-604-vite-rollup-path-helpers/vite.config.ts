import { resolve, join } from "node:path";
import path from "node:path";
import { defineConfig } from "vite";

// Multi-entry Vite config matching zagrajmy/ludamus#287: Rollup inputs are
// declared via path-helper calls rather than string literals.
export default defineConfig({
  build: {
    rollupOptions: {
      input: {
        app: resolve(__dirname, "src/app.ts"),
        modal: path.resolve(__dirname, "src/modal.ts"),
        tabs: join(__dirname, "src/tabs.ts"),
        styles: resolve(__dirname, "src/index.css"),
      },
    },
  },
});
