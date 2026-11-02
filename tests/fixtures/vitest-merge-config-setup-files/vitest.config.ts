import { defineConfig, mergeConfig } from "vitest/config";
import baseConfig from "./vite.config.base.ts";

export default mergeConfig(
  { ...baseConfig, test: {} },
  defineConfig({
    test: {
      setupFiles: ["./src/test-utils/git.setup.ts", "./src/test-utils/env.setup.ts"],
    },
  }),
);
