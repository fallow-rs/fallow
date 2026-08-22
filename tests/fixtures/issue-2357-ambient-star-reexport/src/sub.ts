export * from './sub-deep';
export * as nested from './sub-nested';
export const subOne = (): number => 7;
export interface SubPair {
  c: number;
}
export const SubPair = (): SubPair => ({ c: 3 });
export default function subDefault(): void {}
