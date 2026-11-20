const loadPrices = (): Record<string, number> => ({ shirt: 20, socks: 5 });

export const priceOf = (sku: string): number => {
  const prices = loadPrices();
  return prices[sku] ?? 0;
};
