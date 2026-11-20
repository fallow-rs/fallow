type Line = { price: number; onSale: boolean };

export const applyDiscount = (lines: Line[], member: boolean): number => {
  let total = 0;
  if (member) {
    for (const line of lines) {
      if (line.onSale) {
        continue;
      }
      if (line.price > 100 && lines.length > 3) {
        total += line.price * 0.1;
      }
    }
  }
  return total;
};
