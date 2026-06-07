export const safe = (req: { query: { value: string } }): boolean => {
  return /^[a-z]+$/.test(req.query.value);
};
