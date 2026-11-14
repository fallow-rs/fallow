/** @type {import('stylelint').Config} */
export default {
  extends: ["stylelint-config-example", "stylelint-config-example-order", "./base.cjs"],
  plugins: ["stylelint-plugin-example", "@acme/stylelint-plugin-tokens/rules", "./local-plugin.js"],
  rules: {
    "example/no-magic-values": true,
  },
  overrides: [
    {
      files: ["**/*.html"],
      customSyntax: "postcss-example-html",
    },
  ],
};
