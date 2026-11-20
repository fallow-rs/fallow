export interface CallBox {
  value: string;
}

// A parameter type does not reach the type of the call result.
export interface CallParam {
  flag: boolean;
}

function makeCallBox(_param?: CallParam): CallBox {
  return { value: "a" };
}

export const callStatus = { latency: makeCallBox() };
