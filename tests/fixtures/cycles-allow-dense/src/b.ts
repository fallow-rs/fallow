import { a } from './a';
import { c } from './c';
import { d } from './d';
import { e } from './e';

export function b(): string {
  return [typeof a, typeof c, typeof d, typeof e].join();
}
