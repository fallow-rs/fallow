// FALSE-POSITIVE NEGATIVE: this barrel re-exports a "use client" origin
// (./Widget) alongside an ORDINARY undirected utility (./format). There is no
// server-only origin, so this MUST NOT flag.
export { Widget } from "./Widget";
export { formatDate } from "./format";
