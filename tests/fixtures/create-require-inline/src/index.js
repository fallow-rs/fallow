import { createRequire, createRequire as makeRequire } from 'node:module';

import { loadHoisted } from './hoisted.js';

export const inlineLoad = createRequire(import.meta.url)('inline-load');
export const inlineSubpath = createRequire(import.meta.url)('inline-subpath/lib/index.js');
export const inlineChained = createRequire(import.meta.url)('inline-chained').parser(true);
export const inlineResolve = createRequire(import.meta.url).resolve('inline-resolve');

const req = createRequire(import.meta.url);
export const renamedLoad = req('renamed-load');
export const renamedResolve = req.resolve('renamed-resolve');

export const aliasedImport = makeRequire(import.meta.url)('aliased-import');

export const otherBase = createRequire('/somewhere/else/x.js')('other-base-pkg');

const whole = createRequire(import.meta.url)('./whole.js');
export const wholeA = whole.a;

const { c } = createRequire(import.meta.url)('./destructured.js');
export const destructuredC = c;

createRequire(import.meta.url)('./missing.js');
createRequire(import.meta.url)('not-listed-pkg');

export { loadHoisted };
