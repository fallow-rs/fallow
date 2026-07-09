import type { ImportedDep } from './dep';

class Opts {
  constructor(public c: ImportedDep) {}
}

export class User {
  constructor(private opts: Opts) {}
  run(): void {
    this.opts.c.viaLocalOpts();
  }
}

export const makeUser = (dep: ImportedDep): User => new User(new Opts(dep));
