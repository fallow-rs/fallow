export function orphan(values) {
  if (values.length === 0) {
    return 0;
  }
  if (values.length === 1) {
    return values[0];
  }
  return values.reduce((sum, value) => sum + value, 0);
}
