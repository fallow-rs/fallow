// Positive: response headers derived from a non-literal value are header-injection candidates.
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

export function reflectHeader(res: ResponseLike, value: string): void {
  res.setHeader("X-User", value);
}

export function writeHeaders(res: ResponseLike, headers: Record<string, string>): void {
  res.writeHead(302, headers);
}

export const writeHeadersWithStatusMessage = (
  res: ResponseLike,
  statusMessage: string,
  headers: Record<string, string>,
): void => {
  res.writeHead(302, statusMessage, headers);
};

export const writeComputedHeader = (res: ResponseLike, name: string): void => {
  res.writeHead(302, { [name]: "dynamic" });
};

export const writeSpreadHeaders = (
  res: ResponseLike,
  headers: Record<string, string>,
): void => {
  res.writeHead(302, { ...headers });
};

export const writeGetterHeaders = (res: ResponseLike, value: string): void => {
  res.writeHead(302, {
    get "X-Dynamic"() {
      return value;
    },
  });
};

export const writeNestedHeaders = (res: NestedResponseLike): void => {
  res.writeHead(302, { "X-Nested": { value: "static" } });
};
