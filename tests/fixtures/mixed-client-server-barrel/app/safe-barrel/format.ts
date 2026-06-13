// An ORDINARY utility module: no "use client", no "use server", no server-only
// import. Re-exporting this alongside a client component is completely normal
// and MUST NOT flag (the load-bearing false-positive guard).
export function formatDate(value: number) {
  return String(value);
}
