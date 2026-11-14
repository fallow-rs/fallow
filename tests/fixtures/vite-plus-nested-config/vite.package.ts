import { defineConfig } from "vite-plus";

export const packageConfig = (name: string) =>
  defineConfig({ pack: { entry: ["src/index.ts"] }, test: { name } });
