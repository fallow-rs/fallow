export class LazyThis {
  static #instance = null;
  createScene(): void {}
  unusedLazyThis(): void {}
  static getInstance() {
    if (this.#instance === null) {
      this.#instance = new this();
    }
    return this.#instance;
  }
}

export const lazyThis = LazyThis.getInstance();
