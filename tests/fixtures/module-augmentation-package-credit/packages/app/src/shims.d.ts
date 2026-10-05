declare module 'ambient-lib' {
  export const x: number;
}

declare module '*.svg' {
  const s: string;
  export default s;
}
