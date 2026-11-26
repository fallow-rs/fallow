import { action, computed, makeObservable, observable } from "mobx";

export class FilterStore {
  filters: string[] = [];

  constructor() {
    makeObservable(this, {
      filters: observable,
      issueFilters: computed,
      cleanup: action,
      "quotedAction": action,
    });
  }

  get issueFilters(): string[] {
    return this.filters;
  }

  cleanup = (): void => {
    this.filters = [];
  };

  quotedAction(): void {}

  unannotatedMethod(): void {}
}
