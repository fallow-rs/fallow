export function openDocs(slug: string): Window | null {
  return window.open(
    `https://docs.example.com/search?q=${encodeURIComponent(slug)}`,
    "_blank",
    "noopener,noreferrer",
  );
}
