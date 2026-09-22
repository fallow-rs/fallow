const ORIGIN = "https://api.example.com";
type Input = { query: { url: string; origin: string; host: string } };

export const guarded = (req: Input): Promise<Response> => {
  const destination = new URL(req.query.url);
  if (destination.origin !== ORIGIN) throw new Error("Unsupported origin"); // observed guard
  return fetch(destination.href);
};

export const mutated = (req: Input): Promise<Response> => {
  const destination = new URL(req.query.url);
  if (destination.origin !== ORIGIN) throw new Error("Unsupported origin"); // observed guard
  destination.hostname = req.query.host;
  return fetch(destination.href);
};

export const unrelated = (req: Input): Promise<Response> => fetch(req.query.url);

export const checkedLater = (req: Input): Promise<Response> => {
  const destination = new URL(req.query.url);
  const result = fetch(destination.href);
  if (destination.origin !== ORIGIN) throw new Error("Unsupported origin"); // observed guard
  return result;
};

export const customObject = (req: Input): Promise<Response> => {
  const destination = { origin: req.query.origin, href: req.query.url };
  if (destination.origin !== ORIGIN) throw new Error("Unsupported origin"); // observed guard
  return fetch(destination.href);
};
