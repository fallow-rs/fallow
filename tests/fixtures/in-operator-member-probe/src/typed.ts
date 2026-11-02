import type { ProbeResult } from "./probe";

export class TypedReport {
  get displayDetail(): string {
    return "a type-only import is no neighbor link";
  }
}

export const toResult = (value: string): ProbeResult => value;
