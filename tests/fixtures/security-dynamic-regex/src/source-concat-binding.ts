type RequestLike = {
  query: {
    pattern: string;
  };
};

export function buildFromRequest(req: RequestLike): RegExp {
  const pattern = "^" + req.query.pattern + "$";
  return new RegExp(pattern);
}
