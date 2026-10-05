export default {
  name: "expo-router-app-config-plugins",
  plugins: [
    "expo-router",
    ["@sample/expo-plugin-pdf", {}],
    "sample-expo-plugin-camera",
    ["./plugins/with-sample-setting.ts", { enabled: true }],
  ],
};
