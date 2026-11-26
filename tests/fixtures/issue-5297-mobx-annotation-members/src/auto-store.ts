import { action, makeAutoObservable, makeObservable } from "mobx";

export class AutoStore {
  constructor() {
    makeAutoObservable(this, { overriddenMethod: action.bound, excludedMethod: false });
  }

  static configure(): void {
    makeObservable(this, { staticContextMethod: action });
  }

  overriddenMethod(): void {}

  excludedMethod(): void {}

  staticContextMethod(): void {}

  autoUnusedMethod(): void {}
}
