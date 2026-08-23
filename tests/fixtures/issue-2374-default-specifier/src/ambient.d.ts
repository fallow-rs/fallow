declare module 'untyped-default' {
  export { default } from './ambient-impl';
}

declare module 'untyped-mixed' {
  export { default as Impl, Y as Z } from './ambient-mixed';
}
