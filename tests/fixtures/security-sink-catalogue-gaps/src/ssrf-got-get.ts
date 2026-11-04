import got from "got";

export async function send(url: string): Promise<void> {
  await got.get(url);
}
