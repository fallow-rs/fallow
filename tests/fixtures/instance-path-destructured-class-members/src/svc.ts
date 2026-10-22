export class Svc {
  viaImport(): void {}
  viaImportAssignment(): void {}
  viaImportDefault(): void {}
  viaImportTypeof(): void {}
  viaThis(): void {}
  viaThisAssignment(): void {}
  viaThisDefault(): void {}
  dead(): void {}
}

class Spread {
  first(): void {}
  second(): void {}
}

export const shared = new Svc();
export const spread = new Spread();

export class Shadowed {
  used(): void {}
  hidden(): void {}
}

export class Field {
  first(): void {}
  second(): void {}
}

export const quiet = new Shadowed();
