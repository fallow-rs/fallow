import type { OuterOpts } from './outer';

export class MultiHopUser {
  constructor(private opts: OuterOpts) {}
  run(): void {
    this.opts.mid.leaf.deepM();
  }
}
