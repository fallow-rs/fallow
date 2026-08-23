// A `declare module` body in a plain `.ts` file nothing imports. The shim is
// an unused file, but the module id it declares is imported from outside this
// graph, so the chain behind it keeps its credit and re-enters modules the
// entry point imports directly.
declare module 'unreached-star' {
  export * from './unreached-barrel';
}
