export class Nullish {
  static #instance?: Nullish;
  createScene(): void {}
  unusedNullish(): void {}
  static getInstance() {
    return (Nullish.#instance ??= new Nullish());
  }
}

export const nullish = Nullish.getInstance();
