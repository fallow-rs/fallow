// The entry surface forwards `sub` (a namespace object) through plain
// `export *`, exposes `top` directly, names `named` and the renamed `rn`
// through named re-export chains, and reaches a barrel that re-exports itself
// through an `export *` / `export * as` cycle. It also reaches two barrels
// whose namespace object is named `default`, which no plain `export *`
// forwards, and one barrel whose star-forwarded name is shadowed locally.
export * from './barrel';
export * as top from './top';
export * as default from './entry-default-sub';
export * from './cycle-a';
export * from './star-default';
export * from './named-default-mid';
export { shimOne } from './shim';
export { named } from './named';
export { renamed } from './rename-mid';
export { shadowNs } from './shadow-barrel';
