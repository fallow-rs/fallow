export function sendWildcardMessage(): void {
  window.parent.postMessage({ status: "ready" }, "*");
}
