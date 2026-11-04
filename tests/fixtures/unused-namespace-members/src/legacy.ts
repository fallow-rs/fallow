export namespace Legacy {
  export const used = 1;
  export const aliased = 2;
  export const destructured = 3;
  export const unusedValue = 4;
  export function unusedFn(): void {}
}

export namespace Outer {
  export namespace Inner {
    export const deep = 1;
    export const unusedDeep = 2;
  }
  export namespace Whole {
    export const a = 1;
    export const b = 2;
  }
  export namespace UnusedNested {
    export const c = 1;
  }
}

export class Merged {}
export namespace Merged {
  export const helper = 1;
  export const unusedHelper = 2;
}

export enum Color {
  Red,
  Green,
}
export namespace Color {
  export function parse(value: string): Color {
    return value === "red" ? Color.Red : Color.Green;
  }
  export const unusedColorHelper = 1;
}

export function make(): void {}
export namespace make {
  export const defaults = {};
  export const unusedDefaults = {};
}

export declare namespace Typed {
  export type Shape = { a: number };
  export type UnusedShape = { b: number };
}

export namespace Whole {
  export const a = 1;
  export const b = 2;
}

export namespace Spread {
  export const a = 1;
}

export namespace Reexported {
  export const a = 1;
}

export namespace Body {
  const local = 1;
  export const helper = local + 1;
  export function entry(): number {
    return helper;
  }
}

declare global {
  namespace GlobalNs {
    export const g: number;
  }
}

declare module "external-mod" {
  export namespace Aug {
    export const a: number;
  }
}

namespace LocalOnly {
  export const l = 1;
}
