"use client";

// `metadata` is a Next.js route-segment config export that Next rejects from a
// "use client" file (the basis of `invalid-client-export`). Those illegal names
// are Next ROUTE-SEGMENT config, NOT universal RSC semantics, so under a
// non-Next RSC bundler (Waku) this must NOT be flagged.
export const metadata = { title: "Home" };

export default function Widget() {
  return <span>widget</span>;
}
