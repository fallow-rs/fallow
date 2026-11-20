export const cartTotal = (prices: number[]): number =>
  prices.reduce((sum, price) => sum + price, 0);
