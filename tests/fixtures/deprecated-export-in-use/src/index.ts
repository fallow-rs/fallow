import { alive } from './lib/unused';
import { guardedFoo, guardedString } from './lib/guards';
import { suppressedOld } from './lib/suppressed';
import { offOld } from './lib/off/legacy';
import { other } from './lib/internal-barrel';
import * as ns from './lib/namespace';
import { firstNoSemi, secondNoSemi } from './lib/nosemi';
import './consumers/c01';
import './consumers/c02';
import './consumers/c03';
import './consumers/c04';
import './consumers/c05';
import './consumers/c06';
import './consumers/c07';
import './consumers/c08';
import './consumers/c09';
import './consumers/c10';
import './consumers/c11';
import './consumers/c12';

export { publicOld } from './lib/public';
export * from './lib/mid-barrel';

/** @deprecated Use `entryNew`. */
export const entryOld = 1;

/** @deprecated No internal consumer. */
export const entryUnusedOld = 2;

export const entryNew = 3;

export const run = (): number =>
  firstNoSemi + secondNoSemi + alive + guardedFoo + guardedString + suppressedOld + offOld + other + ns.nsOld;
