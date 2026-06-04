// Positive: response headers derived from a non-literal value are header-injection candidates.
interface ResponseLike {
  setHeader(name: string, value: string): void;
  writeHead(status: number, headers: Record<string, string>): void;
}

export function reflectHeader(res: ResponseLike, value: string): void {
  res.setHeader("X-User", value);
}

export function writeHeaders(res: ResponseLike, headers: Record<string, string>): void {
  res.writeHead(302, headers);
}
