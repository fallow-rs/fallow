export const one = 1;

const flagOne = process.env.FEATURE_ONE;

// fallow-ignore-next-line unused-export
export const ignoredOne = flagOne ? 1 : 0;

export function evaluateOne(input: string): unknown {
  return eval(input);
}
