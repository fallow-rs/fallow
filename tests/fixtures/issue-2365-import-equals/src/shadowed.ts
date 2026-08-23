export import ShadowedTarget = require('./shadowed-target');

export const readShadowed = (): number => Object.values(ShadowedTarget).length;

export const parseShadowed = (ShadowedTarget: { n: number }): number => ShadowedTarget.n;
