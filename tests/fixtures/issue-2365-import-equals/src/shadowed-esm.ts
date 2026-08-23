import * as ShadowedEsmTarget from './shadowed-esm-target';

export const readShadowedEsm = (): number => Object.values(ShadowedEsmTarget).length;

export const parseShadowedEsm = (ShadowedEsmTarget: { n: number }): number => ShadowedEsmTarget.n;
