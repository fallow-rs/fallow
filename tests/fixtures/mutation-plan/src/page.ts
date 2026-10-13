export const revalidate = 60 * 5;

export const config = {
  ttl: 1 + 1,
  compute: (value: number): number => value * 3,
};

export function render(): string {
  return revalidate > 0 ? "cached" : "dynamic";
}
