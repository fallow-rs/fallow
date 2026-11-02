// Positive: request input next to an environment value in one pattern is still
// a source-backed candidate.
type RequestLike = {
  query: {
    pattern: string;
  };
};

export const fromEnvAndRequest = (req: RequestLike): RegExp =>
  new RegExp(process.env.PATTERN_PREFIX + req.query.pattern);
