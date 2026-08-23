// An ambient star hands the whole ES star surface of its target to the
// declared module's consumers, so ambient-barrel joins the exposed namespace
// closure at full namespace-object exposure.
declare module 'ambient-star' {
  export * from './ambient-barrel';
}

export {};
