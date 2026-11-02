export const compile = (names: string[], body: string): unknown => {
  const F = Function;
  const factory = new F(...names, `return (input) => {\n${body}\n}`);
  return factory;
};
