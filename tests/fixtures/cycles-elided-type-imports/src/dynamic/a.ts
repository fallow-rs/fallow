export const loadB = async (): Promise<unknown> => {
  const { kind } = await import('./b');
  return kind;
};
export interface A { name: string }
