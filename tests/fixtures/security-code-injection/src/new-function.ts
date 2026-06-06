export function buildFunction(body: string): Function {
  return new Function(body);
}
