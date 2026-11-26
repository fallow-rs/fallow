import { B } from './b';
export interface A { b?: B }
export const makeA = (): A => ({});
