export const allocate = (req: { query: { count: number } }): unknown[] => {
  const count = req.query.count;
  return new Array(count);
};
