import type { SharedOpts } from './barrel';

export class BarrelUser {
  constructor(private opts: SharedOpts) {}
  run(): void {
    this.opts.c.viaShared();
  }
}
