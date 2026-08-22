// The entry surface forwards `sub` (a namespace object) through plain
// `export *`, exposes `top` directly, and reaches a barrel that re-exports
// itself through an `export *` / `export * as` cycle.
export * from './barrel';
export * as top from './top';
export * from './cycle-a';
export { shimOne } from './shim';
