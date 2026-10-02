export const opener = import.meta.resolve('open-pkg');
export const data = import.meta.resolve('@scope/data/package.json');
export const deep = import.meta.resolve('deep-pkg/dist/x.js');

const meta = { resolve: (specifier) => specifier };
export const local = meta.resolve('local-meta-pkg');
