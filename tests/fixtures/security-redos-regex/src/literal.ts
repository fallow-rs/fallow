export const literal = (req: { query: { value: string } }): boolean => {
  return /^(a+)+$/.test(req.query.value);
};
