// The local `shadowNs` shadows the star-forwarded one, so the entry's
// `shadowNs` is this number and shadow-source's namespace object is unreachable.
export * from './shadow-source';
export const shadowNs = 1;
