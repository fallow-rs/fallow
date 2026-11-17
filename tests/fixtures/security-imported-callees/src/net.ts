export function request(url: string): Promise<Response> {
  return fetch(url);
}
