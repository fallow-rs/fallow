// A reachable non-entry barrel: its namespace re-export is not on the entry
// surface and nothing consumes `hidden`, so none of hidden.ts is credited.
export * as hidden from './hidden';
export const shimOne = 1;
