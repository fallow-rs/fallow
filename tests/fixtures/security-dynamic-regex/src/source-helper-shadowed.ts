type RequestLike = {
  query: {
    pattern: string;
  };
};

function patternFromRequest(req: RequestLike): string {
  return req.query.pattern;
}

export function buildFromShadowedHelper(
  patternFromRequest: (req: RequestLike) => string,
  req: RequestLike,
): RegExp {
  const pattern = patternFromRequest(req);
  return new RegExp(pattern);
}
