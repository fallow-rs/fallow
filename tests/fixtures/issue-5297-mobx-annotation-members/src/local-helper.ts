const makeObservable = (target: object, annotations: object): void => {
  void target;
  void annotations;
};

export class LocalHelperStore {
  constructor() {
    makeObservable(this, { localHelperMethod: true });
  }

  localHelperMethod(): void {}
}
