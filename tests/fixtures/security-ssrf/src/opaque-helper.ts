declare function resolveUrl(token: string): string;

export async function loadOpaque(token: string): Promise<Response> {
  return fetch(resolveUrl(token));
}
