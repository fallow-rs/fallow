// A non-public `import.meta.env` (Vite) secret reaching a network call body:
// the same secret-to-network shape via the import.meta.env source (#890).
const apiKey = import.meta.env.SERVER_API_KEY;
export async function send(destination: string) {
  await fetch(destination, { headers: { authorization: apiKey } });
}
