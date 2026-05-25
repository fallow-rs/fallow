import { defineConfig } from "oxlint";

export default defineConfig({
  jsPlugins: [
    { name: "regexp", specifier: "eslint-plugin-regexp" },
    "./plugins/local-plugin.js",
  ],
});
