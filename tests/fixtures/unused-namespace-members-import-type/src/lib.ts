export namespace Legacy {
  export const used = 1;
  export const unread = 2;
  export const viaBarrel = 3;
}

export namespace Shapes {
  export type Point = { x: number };
  export type Unused = { y: number };
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
}
