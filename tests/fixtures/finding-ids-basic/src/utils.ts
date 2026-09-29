export const used = 1;

export const helper = (): number => 1;

export function unusedFn(): number {
  return 2;
}

export type Shape = { width: number };

export const Dual = 1;
export type Dual = number;

export enum Status {
  Active = "active",
  Retired = "retired",
}

export class Service {
  start(): number {
    return 1;
  }

  stop(): number {
    return 0;
  }

  static reset(): void {}

  reset(): void {}
}
