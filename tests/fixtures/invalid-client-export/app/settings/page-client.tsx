"use client";

// Not a route file: the sibling page.tsx imports the default export. Next.js
// reads route segment config only from page, layout and route files.
export const dynamic = "force-dynamic";

export default function SettingsClient() {
  return <div>Settings</div>;
}
