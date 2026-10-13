export function lonely(value) {
  if (value > 10) {
    return "big";
  }
  if (value > 5) {
    return "medium";
  }
  return "small";
}
