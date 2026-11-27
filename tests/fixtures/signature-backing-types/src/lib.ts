export type NeverUsed = { a: number };

export type UsedInSignature = { b: number };

export type UsedInBody = { c: number };

export type UsedAsGenericArg = { d: number };

function box<T>(v: T) {
  return v;
}

export const used = (x: UsedInSignature) => {
  const y: UsedInBody = { c: x.b };
  return y.c;
};

export const usedGeneric = box<UsedAsGenericArg>({ d: 1 });
