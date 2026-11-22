import { viaA } from './a';
import { shared } from './shared';
import * as all from './hub';

export const start = (): number => viaA() + shared() + all.hubValue;
export { read } from './callers/zed';
