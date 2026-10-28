import { defineConfig } from "oxlint";

export default defineConfig({
  jsPlugins: [
    "./plugins/dir-plugin",
    "./plugins/index-plugin",
    "./plugins/bare-plugin",
  ],
});
