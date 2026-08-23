// The entry reaches this barrel through a plain `export *`. The barrel
// declares its own `starShadowNs`, which shadows the star-forwarded one, so
// star-shadow-target's namespace object never reaches the entry surface.
export * from './star-shadow-source';
export const starShadowNs = 1;
