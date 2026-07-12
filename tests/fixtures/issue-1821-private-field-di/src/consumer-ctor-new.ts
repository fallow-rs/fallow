import { DepCtorNew } from './dep';

// Bare `#`-private field assigned `this.#dep = new Dep()` in the constructor
// (Fix A3 assignment arm; no field type annotation).
export class ConsumerCtorNew {
  #dep;

  constructor() {
    this.#dep = new DepCtorNew();
  }

  run(): void {
    this.#dep.ctorNewM();
  }
}
