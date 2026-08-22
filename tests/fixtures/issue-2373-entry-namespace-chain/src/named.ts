// Reached from the entry by name, not by a plain star: only `named` is on the
// entry surface, so `namedBarrelOne` keeps reporting.
export * as named from './named-sub';
export const namedBarrelOne = 1;
