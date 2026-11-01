import { createRequire } from 'node:module';

export function loadHoisted() {
  return lateRequire('hoisted-load');
}

const lateRequire = createRequire(import.meta.url);
