import { defineConfig } from "oxlint";

export default defineConfig({
  jsPlugins: ["eslint-plugin-top-level"],
  overrides: [
    {
      files: ["*.ts"],
      jsPlugins: ["eslint-plugin-override-only", "./plugins/local-plugin.js"],
    },
  ],
});
