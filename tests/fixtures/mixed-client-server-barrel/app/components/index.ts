// POSITIVE: this barrel re-exports BOTH a "use client" origin (./Button) and a
// server-only origin (./fetchUser). Importing one name from this barrel drags
// the other's directive context across the React Server Components boundary.
export { Button } from "./Button";
export { fetchUser } from "./fetchUser";
