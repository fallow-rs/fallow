export function makeSessionToken(): string {
  const sessionToken = Math.random().toString(36);
  return sessionToken;
}
