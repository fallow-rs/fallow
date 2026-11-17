import { a } from './a';
import { b } from './b';
import { c } from './c';
import { d } from './d';

export function e(): string {
  return [typeof a, typeof b, typeof c, typeof d].join();
}
