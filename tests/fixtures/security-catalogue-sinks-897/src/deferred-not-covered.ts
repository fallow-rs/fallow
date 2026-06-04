// Negative: patterns intentionally DEFERRED from / EXCLUDED by #897 must NOT
// fire, because no matcher row covers them:
//   - document.cookie write + localStorage secret-key (need identifier gating, #892)
//   - res.send(err.stack) info-exposure (needs an error-object shape check)
//   - libxmljs node.find(expr) (excluded: collides with Array.prototype.find)
interface ResponseLike {
  send(body: unknown): void;
}

interface XmlNode {
  find(expr: string): unknown;
}

export function writeCookie(value: string): void {
  document.cookie = value;
}

export function storeToken(token: string): void {
  localStorage.setItem("token", token);
}

export function leakError(res: ResponseLike, err: Error): void {
  res.send(err.stack);
}

export function libxmlFind(node: XmlNode, expr: string): unknown {
  return node.find(expr);
}
