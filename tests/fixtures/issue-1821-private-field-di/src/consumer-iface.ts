import type { IDep } from './dep';

// Interface-typed `#`-private field (Fix A2 typed arm); the analyze layer
// credits every implementer of IDep through interface heritage.
export class ConsumerIface {
  readonly #svc: IDep;

  constructor(svc: IDep) {
    this.#svc = svc;
  }

  run(): void {
    this.#svc.ifaceM();
  }
}
