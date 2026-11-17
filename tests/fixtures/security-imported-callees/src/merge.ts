// A shallow copy: it writes only to a fresh local object.
export function merge<T extends object>(obj: T | null | undefined, subset: Partial<T>): T {
  const copy = Object.assign({}, obj);
  for (const k in subset) {
    copy[k] = subset[k];
  }
  return copy as T;
}
