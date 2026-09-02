import type { StorybookConfig } from "@storybook/react-native";

const config: StorybookConfig = {
  stories: ["../src/mobile-case.tsx"],
  deviceAddons: [
    {
      name: "@storybook/addon-ondevice-actions",
      options: { depth: 2 },
    },
  ],
};

export default config;
