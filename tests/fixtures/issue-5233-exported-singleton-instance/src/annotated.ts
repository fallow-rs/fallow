export class Annotated {
  private static instance: Annotated;
  createScene(): void {}
  unusedAnnotated(): void {}
  static getInstance(): Annotated {
    if (!Annotated.instance) {
      Annotated.instance = new Annotated();
    }
    return Annotated.instance;
  }
}

export const annotated = Annotated.getInstance();
