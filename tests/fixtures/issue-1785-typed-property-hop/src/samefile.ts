export class SameFileDep {
  viaSameFile(): void {}
  deadOnSameFile(): void {}
}

interface LocalOpts {
  c: SameFileDep;
}

export class SameFileUser {
  constructor(private opts: LocalOpts) {}
  run(): void {
    this.opts.c.viaSameFile();
  }
}
