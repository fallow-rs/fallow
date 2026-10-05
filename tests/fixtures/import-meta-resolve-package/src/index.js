import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);

export const opener = import.meta.resolve('open-pkg');
export const data = import.meta.resolve('@scope/data/package.json');
export const deep = import.meta.resolve('deep-pkg/dist/x.js');
export const bridge = require.resolve('bridge-pkg/lib/tsc');
export const tool = require.resolve('@scope/tool/dist/cli.js');

const meta = { resolve: (specifier) => specifier };
export const local = meta.resolve('local-meta-pkg');
