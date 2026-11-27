export class NotInstance {
  ping(): void {}
  pong(): void {}
  static describe(): string {
    return "not an instance";
  }
  static self() {
    return this;
  }
}

export const label = NotInstance.describe();
export const notInstanceClass = NotInstance.self();
