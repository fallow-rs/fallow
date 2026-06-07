export const allocateBuffer = (req: { query: { size: number } }): Buffer => {
  const size = req.query.size;
  return Buffer.alloc(size);
};
