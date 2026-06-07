export const sourceFree = (value: string): boolean => {
  return /^(a+)+$/.test(value);
};
