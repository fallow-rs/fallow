"use client";

// Illegal: `metadata` is a Next.js server-only export and cannot live in a
// "use client" file. Next.js throws a build error for this; fallow catches it.
export const metadata = { title: "Home" };

// The default export is the client component itself and is always valid.
export default function Page() {
  return <div>Hello</div>;
}
