// Positive: additional HTTP clients are SSRF candidates when passed non-literal URLs.
import got from "got";
import * as undici from "undici";

export async function requestUrl(url: string): Promise<void> {
  await got(url);
  await undici.request(url);
}
