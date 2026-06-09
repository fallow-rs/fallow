type RequestLike = {
  query: {
    pattern: string;
  };
};

export function buildFromRequest(req: RequestLike): RegExp {
  const options = { pattern: req.query.pattern };
  return new RegExp(options.pattern);
}
