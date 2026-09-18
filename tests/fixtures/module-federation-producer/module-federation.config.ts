import { createModuleFederationConfig } from "@module-federation/enhanced";

export default createModuleFederationConfig({
  name: "checkout",
  filename: "remoteEntry.js",
  exposes: {
    "./Button": "./src/components/Button.tsx",
    "./cart": "./src/cart",
  },
});
