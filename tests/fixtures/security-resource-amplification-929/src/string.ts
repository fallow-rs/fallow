export const repeatValue = (req: { query: { count: number } }): string => {
  const count = req.query.count;
  const value = "x";
  return value.repeat(count);
};
