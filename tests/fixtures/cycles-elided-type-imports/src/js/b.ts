import { makeA } from './a.js';
export interface B { a?: unknown }
export const build = (): unknown => makeA();
