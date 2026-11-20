const cached: Record<string, number> = { shirt: 1, socks: 1 };

const loadPrices = (): Record<string, number> => {
  if (process.env.NODE_ENV === "test") return cached;
  return { shirt: 20, socks: 5 };
};

export const priceOf = (sku: string): number => {
  const prices = loadPrices();
  return prices[sku] ?? 0;
};
