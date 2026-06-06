export function literalPatterns(): RegExp[] {
  return [RegExp("^[a-z]+$"), new RegExp("^[0-9]+$")];
}
