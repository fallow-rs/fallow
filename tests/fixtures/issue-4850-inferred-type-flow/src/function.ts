export interface FunctionBox {
  value: string;
}

export interface ShadowedBox {
  value: string;
}

const functionBox: FunctionBox = { value: "a" };

const shadowed: ShadowedBox = { value: "b" };

export function getFunctionStatus() {
  return { latency: functionBox };
}

// The local `shadowed` hides the module binding, so `ShadowedBox` does not
// reach the return type.
export function getShadowed() {
  const shadowed = 1;
  return shadowed;
}
export const readShadowed = shadowed.value;
