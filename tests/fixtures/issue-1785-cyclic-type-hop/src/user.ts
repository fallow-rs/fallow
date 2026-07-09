import type { TypeA } from './types-a';

export class CyclicUser {
  constructor(private opts: TypeA) {}
  run(): void {
    this.opts.b.a.leaf.deepM();
  }
}
