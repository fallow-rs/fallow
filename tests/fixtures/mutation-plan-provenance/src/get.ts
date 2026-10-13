export function get(object: Record<string, unknown>, key: string): unknown {
  if (key.length === 0) {
    return object;
  }
  return object[key];
}

export function getOr(object: Record<string, unknown>, key: string, fallback: unknown): unknown {
  return get(object, key) ?? fallback;
}
