class Event {
  fire(): void {}
}

export class LocalEvent extends Event {
  override fire(): void {}
}
