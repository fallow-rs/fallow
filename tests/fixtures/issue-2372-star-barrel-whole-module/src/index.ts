import * as ns from './barrel';
import * as narrow from './narrow-barrel';
import * as cyc from './cycle-a';
import { shimAll } from './shim';
import { passedOn } from './passer';
import { reNs } from './re-shim';
import { aliasApi } from './alias';

// Whole-object use of a namespace import observes every name on the barrel's
// namespace object, including names that only arrive through the barrel's
// own `export *` and `export * as` chains.
export const all = Object.values(ns);

// Member access only: narrowing credits `one` and nothing else.
export const narrowOne = narrow.one();

// Access through an object-literal alias: the alias phase credits `aliasOne`
// and nothing else behind the aliased namespace.
export const aliasOne = aliasApi.aliasNs.aliasOne;

// Whole-object use of a barrel that re-exports itself through a chain.
export const cycleAll = Object.keys(cyc);

export const shims = [shimAll, passedOn, reNs];

// Dynamic-import pattern targets hand the consumer each matched module's
// namespace object.
const mods = import.meta.glob('./mods/*.ts');
export const app = Object.keys(mods).length;

const pluginName = 'one';
export const plugin = import(`./plugins/${pluginName}.ts`);

const icons = require.context('./icons', false);
export const iconKeys = icons.keys();
