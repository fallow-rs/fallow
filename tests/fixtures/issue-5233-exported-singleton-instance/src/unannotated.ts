export class Unannotated {
  private static instance: Unannotated | undefined;
  createScene(): void {}
  unusedUnannotated(): void {}
  static getInstance() {
    if (!Unannotated.instance) {
      Unannotated.instance = new Unannotated();
    }
    return Unannotated.instance;
  }
}

const unannotated = Unannotated.getInstance();
export { unannotated };
