import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);

function packageRoot(name) {
  return require.resolve(`${name}/package.json`);
}

export const helperRoot = packageRoot('helper-only-pkg');

const PACKAGES = ['table-only-pkg'];
export const tableRoots = [];
for (const name of PACKAGES) {
  tableRoots.push(require.resolve(`${name}/package.json`));
}

export const searchedRoot = require.resolve('searched-pkg', { paths: [process.cwd()] });
