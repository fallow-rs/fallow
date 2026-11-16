import { a } from './a';
import { b } from './b';
import { d } from './d';
import { e } from './e';

export function c(): string {
  return [typeof a, typeof b, typeof d, typeof e].join();
}
