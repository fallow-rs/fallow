// Writes to a parameter, not to a fresh local object.
export function merge(target: Record<string, unknown>, source: Record<string, unknown>): Record<string, unknown> {
  for (const key in source) {
    target[key] = source[key];
  }
  return target;
}
