export function openRemote(origin: string, slug: string): Window | null {
  return window.open(
    `${origin}/search?q=${encodeURIComponent(slug)}`,
    "_blank",
    "noopener,noreferrer",
  );
}
