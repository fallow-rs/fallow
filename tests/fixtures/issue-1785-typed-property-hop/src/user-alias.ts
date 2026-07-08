import type { AliasDep } from './alias-dep';

type Opts = {
  c: AliasDep;
};

export class UserAlias {
  constructor(private opts: Opts) {}
  run(): void {
    this.opts.c.viaAlias();
  }
}
