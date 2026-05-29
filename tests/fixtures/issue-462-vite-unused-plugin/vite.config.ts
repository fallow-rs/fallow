import Inspect from "vite-plugin-inspect";
import { defineConfig } from "vite";

// vite-plugin-svgr and vite-plugin-eslint are declared in devDependencies but
// NOT used here. Before issue #462 they were credited by an exact-name match in
// the general tooling catalogue, hiding them. Now they must surface as unused.
export default defineConfig({
  plugins: [Inspect()],
});
