export * from './impl-deep';
export * as sub from './sub';
export const helperA = (): number => 1;
export const helperB = (): number => 2;
export interface Merged {
  a: number;
}
export const Merged = (): Merged => ({ a: 1 });
export default function unusedDefault(): void {}
