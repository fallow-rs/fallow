import { resolve } from "node:path";

export default {
  main: {
    build: { lib: { entry: resolve(__dirname, "src/main/app.ts") } },
  },
  preload: {},
};
