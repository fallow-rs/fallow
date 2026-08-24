const hand = (value: unknown): unknown => value;

export const useDynamicObject = async (): Promise<unknown> => {
  const Icons = await import('./dynamic-icons');
  return hand(Icons) ?? Icons.DynamicStar;
};
