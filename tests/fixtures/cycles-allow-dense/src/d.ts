import { a } from './a';
import { b } from './b';
import { c } from './c';
import { e } from './e';

export function d(): string {
  return [typeof a, typeof b, typeof c, typeof e].join();
}
