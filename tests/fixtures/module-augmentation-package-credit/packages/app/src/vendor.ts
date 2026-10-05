import type { Thing } from 'missing-lib';

declare module 'undeclared-lib' {
  interface Extra {
    b: number;
  }
}

export type Wrapped = Thing;
