// The entry reaches this barrel through a plain `export *`, which forwards
// every name except `default`, so this namespace object never lands on the
// entry surface.
export * as default from './star-default-sub';
export const starDefaultOne = 1;
