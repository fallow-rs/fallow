export default {
  framework: "@storybook/react",
  staticDirs: [
    "../src/lib/tokens",
    { from: "../src/lib/icons", to: "icons/" },
    { from: "../../../node_modules/vendor-icons", to: "/vendor/" }
  ]
};
