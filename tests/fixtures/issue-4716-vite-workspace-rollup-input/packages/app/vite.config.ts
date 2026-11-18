import path from "path";
import { defineConfig } from "vite";

export default defineConfig({
  build: {
    rollupOptions: {
      input: { third: "src/third.ts" },
    },
  },
  environments: {
    sw: {
      build: {
        rollupOptions: {
          input: { second: path.resolve(__dirname, "src/second.ts") },
        },
      },
    },
  },
});
