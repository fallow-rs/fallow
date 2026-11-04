export function only(input: string): string;
export function only(input: number): number;
export function only(input: string | number): string | number {
  return input;
}
