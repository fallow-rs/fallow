// Negatives that must NOT fire.
// (1) Public-by-convention env var (build-inlined, not a secret).
const base = process.env.NEXT_PUBLIC_API_BASE;
export async function loadConfig() {
  await fetch(base);
}
// (2) Co-occurrence only: reads a secret AND calls fetch, but the secret never
// flows into the request (no same-identifier flow).
const secret = process.env.OTHER_SECRET;
export async function unrelated() {
  console.log(secret.length);
  await fetch("https://example.com/health");
}
// (3) Public Vite env var (VITE_ prefix is build-inlined, not a secret).
const vitePublic = import.meta.env.VITE_PUBLIC_URL;
export async function loadVite() {
  await fetch(vitePublic);
}
