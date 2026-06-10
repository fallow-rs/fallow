export async function loadRemote(origin: string, token: string): Promise<Response> {
  return fetch(`${origin}/v1/invites/${encodeURIComponent(token)}`);
}
