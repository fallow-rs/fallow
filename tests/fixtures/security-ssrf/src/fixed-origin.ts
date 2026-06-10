const API_URL = "https://api.example.com";

export async function loadInvite(token: string): Promise<Response> {
  return fetch(`${API_URL}/v1/invites/${encodeURIComponent(token)}`);
}
