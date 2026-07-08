import type { OptDep } from './dep';

interface Opts {
  c: OptDep;
}

export class UserInterface {
  constructor(private opts: Opts) {}
  run(): void {
    this.opts.c.optM();
  }
}
