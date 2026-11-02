interface RequestLike {
  query: { first: string; second: string };
}

export const patterns = (req: RequestLike) => {
  const { first, second } = req.query;
  return [new RegExp(first), new RegExp(second)];
};
