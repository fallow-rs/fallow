type RequestLike = {
  query: {
    pattern: string;
  };
};

function patternFromRequest(req: RequestLike): string {
  return req.query.pattern;
}

function wrappedPattern(req: RequestLike): string {
  return patternFromRequest(req);
}

export function buildFromSecondHopHelper(req: RequestLike): RegExp {
  const pattern = wrappedPattern(req);
  return new RegExp(pattern);
}
