import type { SharedOpts } from './opts';

export class DirectUser {
  constructor(private opts: SharedOpts) {}
  run(): void {
    this.opts.c.viaShared();
  }
}
