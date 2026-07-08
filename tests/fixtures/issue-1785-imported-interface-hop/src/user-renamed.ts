import type { RenamedOpts } from './opts-renamed';

export class RenamedUser {
  constructor(private opts: RenamedOpts) {}
  run(): void {
    this.opts.c.viaRenamed();
  }
}
