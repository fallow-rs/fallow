export namespace JSX {
  export interface Element {
    tag: string;
  }
}

export namespace UnusedTypes {
  export interface Thing {
    a: number;
  }
}

export namespace UnusedRuntime {
  export const version = '1';
}

export function merged(): number {
  return 1;
}

export namespace merged {
  export interface Options {
    x: number;
  }
}
