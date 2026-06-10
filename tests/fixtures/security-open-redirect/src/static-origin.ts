const DOCS_URL = "https://docs.example.com";

export function openDocs(): Window | null {
  return window.open(DOCS_URL, "_blank", "noopener,noreferrer");
}
