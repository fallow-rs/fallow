// A server (no "use client") Next.js App Router page.
//
// `metadata` and `default` are real Next.js route exports the framework
// consumes, so they are credited (never unused). `meatdata` is a typo of
// `metadata` and `helper` is a stray non-route export: neither is in the
// nextjs plugin's used-exports allowlist, so under --include-entry-exports
// both surface as unused exports. This is the knip-can't-but-fallow-does gap.
export const metadata = { title: "Home" };

export const meatdata = { title: "Typo" };

export const helper = () => 42;

export default function Page() {
  return <div>{helper()}</div>;
}
