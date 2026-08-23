// The entry surface forwards `sub` (a namespace object) through plain
// `export *`, exposes `top` directly, names `named` and the renamed `rn`
// through named re-export chains, re-exports an imported namespace binding
// under its own name, and reaches a barrel that re-exports itself through an
// `export *` / `export * as` cycle. It also reaches two barrels whose
// namespace object is named `default`, which no plain `export *` forwards,
// and three barrels whose star-forwarded name is shadowed or ambiguous.
import * as bindNs from './bind-barrel';

export * from './barrel';
export * from './star-shadow-barrel';
export * from './ambig-barrel';
export { bindNs };
export * as top from './top';
export * as default from './entry-default-sub';
export * from './cycle-a';
export * from './star-default';
export * from './named-default-mid';
export { shimOne } from './shim';
export { named } from './named';
export { renamed } from './rename-mid';
export { shadowNs } from './shadow-barrel';
