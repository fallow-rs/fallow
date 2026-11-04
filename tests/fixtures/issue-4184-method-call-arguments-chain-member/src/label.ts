export class Label {
  constructor(readonly text: string) {}

  render(): string {
    return this.text;
  }

  hide(): void {}
}
