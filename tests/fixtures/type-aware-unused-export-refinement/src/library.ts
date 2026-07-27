export interface Api<T> {
  value: T;
}

export interface Merged {
  value: string;
}

export namespace Merged {
  export const kind = "merged";
}

export type Complex<T> = T extends Api<infer Value> ? Value : never;

export const runtimeOnly = 1;

export const actuallyUnused = 2;
