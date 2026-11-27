export class Logical {
  static instance = null;
  createScene(): void {}
  unusedLogical(): void {}
  static getInstance() {
    return Logical.instance ?? (Logical.instance = new Logical());
  }
}

export const logical = Logical.getInstance();
