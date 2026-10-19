export function evict(map: Map<number, string>, key: number): boolean {
  if (key < 0) return false;
  map.delete(key);
  return true;
}
