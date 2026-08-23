// Twin of `bare-shadowed.ts`: the same re-exported namespace binding, the same
// shadowing parameter, no whole-object use.
import * as BareShadowedEsmTarget from './bare-shadowed-esm-target';

export { BareShadowedEsmTarget };

export const parseBareShadowedEsm = (BareShadowedEsmTarget: { n: number }): number =>
  BareShadowedEsmTarget.n;
