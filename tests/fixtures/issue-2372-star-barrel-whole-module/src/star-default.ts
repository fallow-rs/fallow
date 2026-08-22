// A plain `export *` never forwards `default`, so this namespace object is
// not on the barrel's namespace object and its chain stays reported.
export * as default from './star-default-sub';
export const starDefaultOne = 1;
