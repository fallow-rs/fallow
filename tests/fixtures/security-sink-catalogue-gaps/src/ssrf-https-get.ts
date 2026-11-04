import https from "node:https";

export function send(url: string): void {
  https.get(url);
}
