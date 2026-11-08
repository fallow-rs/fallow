const memo = new Map<number, number>();

export function double(value: number): number {
  if (!memo.has(value)) memo.set(value, value * 2);
  return memo.get(value) as number;
}

export function stays(): number {
  return 1;
}
