type RequestLike = {
  query: {
    pattern: string;
  };
};

const patternFromRequest = function (req: RequestLike): string {
  return req.query.pattern;
};

export function buildFromFunctionExpressionHelper(req: RequestLike): RegExp {
  const pattern = patternFromRequest(req);
  return new RegExp(pattern);
}
