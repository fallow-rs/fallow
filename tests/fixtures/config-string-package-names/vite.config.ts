import react from '@vitejs/plugin-react-swc';
import wyw from '@wyw-in-js/vite';
import { defineConfig } from 'vite';

const wrap = <T>(plugin: T): T => plugin;

export default defineConfig(() => ({
  plugins: [
    react({
      plugins: [['@lingui/swc-plugin', {}]],
    }),
    wrap(
      wyw({
        babelOptions: {
          presets: ['@babel/preset-typescript'],
        },
      }),
    ),
  ],
}));
