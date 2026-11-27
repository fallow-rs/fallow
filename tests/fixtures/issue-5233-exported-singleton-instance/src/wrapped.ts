export class Wrapped {
  static #instance?: Wrapped;
  createScene(): void {}
  unusedWrapped(): void {}
  static getInstance() {
    if (!Wrapped.#instance) {
      Wrapped.#instance = new Wrapped();
    }
    return Wrapped.#instance!;
  }
}

export const wrapped = Wrapped.getInstance();
