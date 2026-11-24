export namespace Codes {
  export const used = 1;
  export const unusedMember = 2;
}

export enum Level {
  Low = "low",
  High = "high",
}

export class Service {
  run(): number {
    return 1;
  }

  idle(): number {
    return 2;
  }
}
