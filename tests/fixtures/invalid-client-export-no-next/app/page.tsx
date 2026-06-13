"use client";

// Without `next` declared, the "use client" directive carries no special
// meaning and `metadata` is a perfectly legal export name. The detector is
// gated on `next` being present, so this produces zero findings.
export const metadata = { title: "Home" };

export default function Page() {
  return <div>Hello</div>;
}
