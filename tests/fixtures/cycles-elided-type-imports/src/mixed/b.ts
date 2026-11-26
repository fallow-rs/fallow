import { load } from './a';
export interface Config { name: string }
export const defaults: Config = { name: 'x' };
export const reload = (): Config => load();
