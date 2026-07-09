import type { TypeB } from './types-b';
import type { LeafDep } from './leaf';

export interface TypeA {
  b: TypeB;
  leaf: LeafDep;
}
