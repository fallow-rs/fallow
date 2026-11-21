export namespace PubCodes {
  export const used = 1;
  export const unusedMember = 2;
}

export enum PubLevel {
  Low = "low",
  High = "high",
}

export class PubService {
  run(): number {
    return 1;
  }

  idle(): number {
    return 2;
  }
}
