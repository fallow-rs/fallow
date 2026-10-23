// fallow-ignore-next-line unused-export -- tested directly
export function double(n: number): number {
  return n * 2;
}

export function quadruple(n: number): number {
  return double(double(n));
}

// fallow-ignore-next-line unused-export -- used by index and a test
export function triple(n: number): number {
  return n * 3;
}
