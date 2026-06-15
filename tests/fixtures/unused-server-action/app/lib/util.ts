// A plain (non-"use server") module. `deadUtil` is referenced by no consumer,
// so it is an ordinary `unused-export` and must NOT be reclassified as an
// unused-server-action (this file carries no "use server" directive).
export function usedUtil() {
  return 1;
}

export function deadUtil() {
  return 2;
}
