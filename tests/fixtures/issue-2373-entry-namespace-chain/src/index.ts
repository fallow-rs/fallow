// The entry surface forwards `sub` (a namespace object) through plain
// `export *`, exposes `top` directly, names `named` and the renamed `rn`
// through named re-export chains, and reaches a barrel that re-exports itself
// through an `export *` / `export * as` cycle.
export * from './barrel';
export * as top from './top';
export * from './cycle-a';
export { shimOne } from './shim';
export { named } from './named';
export { renamed } from './rename-mid';
