import { b } from './b';
import { c } from './c';
import { d } from './d';
import { e } from './e';

export function a(): string {
  return [typeof b, typeof c, typeof d, typeof e].join();
}
