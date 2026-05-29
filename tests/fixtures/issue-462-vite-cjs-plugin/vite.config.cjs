// CommonJS Vite config. vite-plugin-svgr is loaded via a const require. The
// config file is a discovered source file, so the normal import graph captures
// the require and credits the package (issue #462: vite-plugin-* is no longer
// an exact tooling-catalogue match). vite-plugin-eslint is declared but not
// required here, so it must surface as unused.
const svgr = require("vite-plugin-svgr");
const { defineConfig } = require("vite");

module.exports = defineConfig({
  plugins: [svgr()],
});
