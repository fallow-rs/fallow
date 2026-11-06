export class RectTable {
  setVip(): void {}

  reset(): void {}

  rectOnly(): void {}
}

export class RoundTable {
  setVip(): void {}

  reset(): void {}
}

export type Table = RectTable | RoundTable;

export class Chair {
  narrowed(): void {}

  rectOnly(): void {}
}

export class Seat {
  claim(): void {}

  release(): void {}

  narrowed(): void {}

  factoryLocal(): void {}
}
