type Line = { price: number; quantity: number; taxRate: number };

export const cartTotal = (lines: Line[]): number => {
  let total = 0;
  for (const line of lines) {
    const net = line.price * line.quantity;
    const tax = net * line.taxRate;
    total += Math.round(net + tax);
  }
  return total;
};
