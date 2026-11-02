// Positive: response headers derived from request input are header-injection candidates.
interface ResponseLike {
  setHeader(name: string, value: string): void;
  writeHead(status: number, headers: Record<string, string | string[]>): void;
  writeHead(
    status: number,
    statusMessage: string,
    headers: Record<string, string | string[]>,
  ): void;
}

interface NestedResponseLike {
  writeHead(status: number, headers: Record<string, unknown>): void;
}

interface RequestLike {
  query: Record<string, string>;
  body: Record<string, string>;
}

export function reflectHeader(req: RequestLike, res: ResponseLike): void {
  const value = req.query.user;
  res.setHeader("X-User", value); // candidate
}

export function writeHeaders(req: RequestLike, res: ResponseLike): void {
  const headers = req.body;
  res.writeHead(302, headers); // candidate
}

export const writeHeadersWithStatusMessage = (req: RequestLike, res: ResponseLike): void => {
  const statusMessage = req.query.message;
  const headers = req.body;
  res.writeHead(302, statusMessage, headers); // candidate
};

export const writeComputedHeader = (req: RequestLike, res: ResponseLike): void => {
  const name = req.query.name;
  res.writeHead(302, { [name]: "dynamic" }); // candidate
};

export const writeSpreadHeaders = (req: RequestLike, res: ResponseLike): void => {
  const headers = req.body;
  res.writeHead(302, { ...headers }); // candidate
};

// A getter body is not traced, so this request value is a known false negative.
export const writeGetterHeaders = (req: RequestLike, res: ResponseLike): void => {
  const value = req.query.value;
  res.writeHead(302, {
    get "X-Dynamic"() {
      return value;
    },
  });
};

export const writeNestedHeaders = (res: NestedResponseLike): void => {
  res.writeHead(302, { "X-Nested": { value: "static" } });
};

// Negative: an adapter copies its own header map into the response. No request
// input reaches the value, so it is not a candidate.
export const copyHeaders = (res: ResponseLike, headers: Map<string, string>): void => {
  for (const [headerName, headerValue] of headers) {
    res.setHeader(headerName, headerValue);
  }
};
