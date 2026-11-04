export class RectTable {
  setVip(): void {}

  lock(): void {}

  reset(): void {}
}

export class RoundTable {
  setVip(): void {}

  lock(): void {}
}

export type Table = RectTable | RoundTable;
