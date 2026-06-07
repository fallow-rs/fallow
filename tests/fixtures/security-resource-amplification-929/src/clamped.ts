export const clamped = (req: { query: { count: number } }): string => {
  const count = req.query.count;
  const value = "x";
  return value.repeat(Math.min(count, 1024));
};
