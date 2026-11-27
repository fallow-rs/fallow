export class Remote {
  private static instance: Remote;
  createScene(): void {}
  reset(): void {}
  unusedRemote(): void {}
  static getInstance(): Remote {
    if (!Remote.instance) {
      Remote.instance = new Remote();
    }
    return Remote.instance;
  }
  static describe(): string {
    return "remote";
  }
  static self() {
    return this;
  }
}
