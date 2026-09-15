// The return type is inferred, so only the semantic pass can see that the
// public signature of `buildLib` references the non-exported `LibInternal`.
type LibInternal = { id: string };

const makeLib = (): LibInternal => ({ id: 'lib' });

export const buildLib = () => makeLib();
