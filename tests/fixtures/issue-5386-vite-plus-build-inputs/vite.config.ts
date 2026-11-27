import { defineConfig } from "vite-plus";

export default defineConfig({
  resolve: {
    alias: { "@lib": "/src/lib" },
  },
  build: {
    rollupOptions: { input: ["index.html", "second.html"] },
  },
  environments: {
    worker: {
      build: {
        rolldownOptions: { input: "src/workerMain.ts" },
      },
    },
  },
});
