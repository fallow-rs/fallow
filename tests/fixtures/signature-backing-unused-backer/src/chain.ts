export interface Inner {
  value: number;
}

export interface Outer {
  inner: Inner;
}

export function build(): Outer {
  return { inner: { value: 1 } };
}
