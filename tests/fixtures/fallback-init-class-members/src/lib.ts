export class Cache {
  used(): void {}
  unused(): void {}
}

export class Pool {
  used(): void {}
  unused(): void {}
}

export class Link {
  static open(path: string): Link {
    return new Link();
  }
  used(): void {}
  unused(): void {}
}

export class Queue {
  used(): void {}
  unused(): void {}
}
