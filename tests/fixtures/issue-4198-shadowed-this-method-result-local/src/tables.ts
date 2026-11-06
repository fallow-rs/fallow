export class RectTable {
  setVip(): void {}

  reset(): void {}
}

export class RoundTable {
  setVip(): void {}

  reset(): void {}
}

export type Table = RectTable | RoundTable;

export class Seat {
  claim(): void {}

  release(): void {}
}
