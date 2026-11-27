"use client";

// Next.js reads route handlers only from route files, so a page may use the
// name for an ordinary export.
export const GET = "get";

// Next.js rejects metadata in a client page.
export const metadata = { title: "Report" };

export default function ReportPage() {
  return <div>{GET}</div>;
}
