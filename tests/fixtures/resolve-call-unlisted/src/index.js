import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
export const b = require.resolve('listed-req/package.json');
export const c = require.resolve('unlisted-req/package.json');
export { both } from './both.js';
export { helperRoot, tableRoots, searchedRoot } from './helpers.js';
