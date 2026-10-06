/** @import { Config } from '../types/config' */
/** @import * as shapes from '../types/shapes' */
/** @import * as units from './units' */
/**
 * @import {
 *   Item,
 *   Store as ItemStore,
 * } from './model'
 */
/** @import Settings from './settings' */

/** @type {Config} */
export const config = { name: "app" };

/** @type {shapes.Circle} */
export const circle = { radius: 1 };

/** @type {ItemStore} */
export const store = { items: [] };

/** @type {Item | Settings | null} */
export const selected = null;

/** @type {units.Meter} */
export const width = 1;
