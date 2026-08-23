// Reached through ambient-barrel's plain `export *`, which forwards named
// exports and never `default`, so this namespace object exposes nothing.
export * as default from './ambient-mid-target';
export const ambientMidOne = 1;
