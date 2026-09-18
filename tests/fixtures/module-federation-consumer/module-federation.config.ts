import { createModuleFederationConfig } from "@module-federation/enhanced";

export default createModuleFederationConfig({
  name: "host",
  remotes: {
    checkout: "checkout@https://example.test/remoteEntry.js",
  },
});
