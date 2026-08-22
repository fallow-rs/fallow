export const helperE = (): number => 5;
export interface DeepMerged {
  b: number;
}
export const DeepMerged = (): DeepMerged => ({ b: 2 });
export default function unusedDeepDefault(): void {}
