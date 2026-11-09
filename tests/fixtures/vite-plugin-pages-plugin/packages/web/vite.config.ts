import { defineConfig } from "vite";
import Pages from "vite-plugin-pages";

export default defineConfig({
  plugins: [Pages({ dirs: ["../common/src/pages", { dir: "./src/pages", baseRoute: "" }] })],
});
