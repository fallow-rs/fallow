import { reactRouter } from '@react-router/dev/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  root: __dirname,
  plugins: [reactRouter()],
});
