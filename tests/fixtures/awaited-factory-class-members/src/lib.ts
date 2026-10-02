export class Widget {
  usedAtTopLevel(): void {}
  usedInAsyncFunction(): void {}
  neverUsed(): void {}
}

export async function makeWidget(): Promise<Widget> {
  return new Widget();
}

export class Gauge {
  readValue(): number {
    return 0;
  }
  neverRead(): number {
    return 1;
  }
}

export async function loadGauge(): Promise<Gauge> {
  return {} as Gauge;
}

export class Plain {
  readAfterAwait(): void {}
}

export function makePlain(): Plain {
  return new Plain();
}

export class Pending {
  onlyReadOnPromise(): void {}
}

export async function makePending(): Promise<Pending> {
  return new Pending();
}
