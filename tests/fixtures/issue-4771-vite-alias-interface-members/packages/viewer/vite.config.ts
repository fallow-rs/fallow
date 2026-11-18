import path from "path";
import { defineConfig } from "vite";

export default defineConfig({
  resolve: {
    alias: {
      "@iso": path.resolve(__dirname, "../iso"),
    },
  },
  build: {
    rollupOptions: {
      input: {
        sw: path.resolve(__dirname, "src/sw-main.ts"),
      },
    },
  },
});
