export const two = 1;

const flagTwo = process.env.FEATURE_TWO;

// fallow-ignore-next-line unused-export
export const ignoredTwo = flagTwo ? 1 : 0;

export function evaluateTwo(input: string): unknown {
  return eval(input);
}
