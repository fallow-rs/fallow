export * as shimSub from './shim-sub';
export const helperD = (): number => 4;
export const User = { parse: (value: unknown): unknown => value };
export type User = { id: number };
