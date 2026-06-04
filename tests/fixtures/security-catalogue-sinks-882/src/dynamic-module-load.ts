// Positive: a non-literal CommonJS specifier is a dynamic-module-load candidate.
export function loadPlugin(name: string): unknown {
  return require(name);
}
