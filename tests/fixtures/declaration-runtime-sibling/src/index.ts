import { helper, type HelperOpts } from './lib.mjs';
import legacy, { type LegacyOpts } from './legacy.js';
import * as ns from './ns.mjs';

const opts: HelperOpts = { n: 1 };
const legacyOpts: LegacyOpts = { flag: true };
const wrapOpts: ns.WrapOpts = { w: 1 };

console.log(helper(opts), legacy(legacyOpts), ns.wrap(wrapOpts));
