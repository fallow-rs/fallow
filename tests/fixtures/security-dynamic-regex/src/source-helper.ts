type RequestLike = {
  query: {
    pattern: string;
  };
};

function patternFromRequest(req: RequestLike): string {
  return req.query.pattern;
}

export function buildFromHelper(req: RequestLike): RegExp {
  const pattern = patternFromRequest(req);
  return new RegExp(pattern);
}
