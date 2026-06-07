type RequestLike = {
  query: {
    pattern: string;
  };
};

export function buildFromHoistedHelper(req: RequestLike): RegExp {
  const pattern = patternFromRequest(req);
  return new RegExp(pattern);
}

function patternFromRequest(req: RequestLike): string {
  return req.query.pattern;
}
