import type { Preview } from '@storybook/react-native';

const preview: Preview = {
  parameters: {
    controls: { matchers: { color: /(background|color)$/i } },
  },
};

export default preview;
