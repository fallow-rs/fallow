import { defineConfig } from 'vitepress';

function extendConfig(config: object): object {
  return config;
}

function withMermaid(config: object): object {
  return config;
}

export default extendConfig(
  withMermaid(
    defineConfig({
      title: 'Docs',
      vite: {
        optimizeDeps: {
          include: ['mermaid > @braintree/sanitize-url', 'nested-only'],
          exclude: ['excluded-dep'],
        },
        ssr: {
          external: ['ssr-external-dep'],
          noExternal: ['ssr-dep'],
        },
      },
    }),
  ),
);
