import { defineConfig } from "@rstest/core";

export default defineConfig({
  include: ["**/*.meow.ts"],
  globalSetup: "./global-setup.ts",
});
