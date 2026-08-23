// The shape the shadow guard used to report: an exported import-equals whose
// name a function parameter binds again, and no `Object.values(...)` anywhere
// to mask the difference. `bare-shadowed-esm.ts` is the twin.
export import BareShadowedTarget = require('./bare-shadowed-target');

export const parseBareShadowed = (BareShadowedTarget: { n: number }): number =>
  BareShadowedTarget.n;
