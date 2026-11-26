import { area } from './a';
type Shape = { width: number; label?: ReturnType<typeof area> };
export { Shape };
export namespace Units {
  export type Name = 'px' | 'em';
}
