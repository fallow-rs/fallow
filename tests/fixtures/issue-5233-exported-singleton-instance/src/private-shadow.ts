class Other {}

export class PrivateShadow {
  private static instance: PrivateShadow;
  static #instance = new Other();
  createScene(): void {}
  static getInstance() {
    return this.#instance;
  }
}

export const privateShadow = PrivateShadow.getInstance();
