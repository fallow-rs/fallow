import { helper, type HelperOpts } from './lib.mjs';
import legacy, { type LegacyOpts } from './legacy.js';

const opts: HelperOpts = { n: 1 };
const legacyOpts: LegacyOpts = { flag: true };

console.log(helper(opts), legacy(legacyOpts));
