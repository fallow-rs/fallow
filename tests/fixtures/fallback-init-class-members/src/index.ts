import { Cache, Link, Pool, Queue } from './lib';

interface Opts {
  cache?: Cache;
}

export function a(o: Opts): void {
  const c = o.cache ?? new Cache();
  c.used();
}

export function b(x: any): void {
  const p = x || new Pool();
  p.used();
}

export function c(f: boolean): void {
  const l = f ? Link.open('p') : undefined;
  l?.used();
}

export function d(f: boolean): void {
  const q = f ? new Queue() : null;
  q?.used();
}
