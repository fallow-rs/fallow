import { resolve } from "node:path";

export default {
  main: {
    build: { rollupOptions: { input: { app: resolve(__dirname, "src/main/app.ts") } } },
  },
  preload: {},
};
