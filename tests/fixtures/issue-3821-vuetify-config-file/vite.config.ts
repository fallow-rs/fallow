import { defineConfig } from "vite";
import vuetify from "vite-plugin-vuetify";

export default defineConfig({
  plugins: [vuetify({ styles: { configFile: "src/styles/settings.scss" } })],
});
