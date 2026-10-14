function scale(value: number): number {
  return value * 2;
}

export function price(value: number): number {
  return scale(value) + 1;
}
