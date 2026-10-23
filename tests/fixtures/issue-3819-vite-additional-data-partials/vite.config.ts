import { defineConfig } from "vite";

export default defineConfig({
  css: {
    preprocessorOptions: {
      scss: {
        additionalData:
          '@use "./src/styles/tokens";\n@use "./src/styles/theme";\n@use "src/styles/mixins";\n',
      },
    },
  },
});
