import { default as react } from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [
    react({
      babel: {
        plugins: ["babel-plugin-plain"],
        presets: [["@babel/preset-react", { runtime: "automatic" }]],
      },
    }),
  ],
  test: {
    include: ["src/**/*.test.ts"],
  },
});
