const {
  ModuleFederationPlugin,
} = require("@module-federation/enhanced/webpack");
const mfConfig = require("./module-federation.config");

module.exports = {
  entry: "./src/index.ts",
  plugins: [new ModuleFederationPlugin(mfConfig)],
};
