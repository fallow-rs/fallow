// An Angular-style HttpClient named `http`, without the node:http import.
interface HttpClient {
  get(url: string): unknown;
}

export function load(http: HttpClient, url: string): unknown {
  return http.get(url);
}
