export function formatCount(count) {
  if (count === 0) {
    return "none";
  }
  if (count === 1) {
    return "one";
  }
  return "many";
}
