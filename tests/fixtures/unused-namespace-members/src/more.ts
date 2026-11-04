export namespace Api {
  export namespace Types {
    export type Request = { id: string };
    export type UnusedResponse = { ok: boolean };
  }
  export namespace Escaped {
    export const x = 1;
    export const y = 2;
  }
  namespace Hidden {
    export const h = 1;
  }
  export const usesHidden = Hidden.h;
  // fallow-ignore-next-line unused-namespace-member
  export const suppressed = 1;
  export const viaNamespaceImport = 1;
  export const viaTypeof = 1;
}

export namespace Split {
  export const first = 1;
}
export namespace Split {
  export const second = first + 1;
}

export namespace Dotted.Inner {
  export const used = 1;
  export const unusedDotted = 2;
}
