// Negative control: a SERVER file (no "use client" directive) exporting
// `metadata`. This is the legitimate Next.js pattern and must never be flagged.
export const metadata = { title: "Server" };

export default function ServerThing() {
  return null;
}
