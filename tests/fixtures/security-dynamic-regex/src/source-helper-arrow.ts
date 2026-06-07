type RequestLike = {
  query: {
    pattern: string;
  };
};

const patternFromRequest = (req: RequestLike): string => req.query.pattern;

export function buildFromArrowHelper(req: RequestLike): RegExp {
  const pattern = patternFromRequest(req);
  return new RegExp(pattern);
}
