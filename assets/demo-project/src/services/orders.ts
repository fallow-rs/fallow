type Line = { price: number; quantity: number; discount?: number; taxRate?: number };

export const orderTotal = (lines: Line[], country: string, coupon?: string): number => {
  let total = 0;
  for (const line of lines) {
    const gross = line.price * line.quantity;
    const discounted = line.discount ? gross - gross * line.discount : gross;
    const taxed = line.taxRate ? discounted + discounted * line.taxRate : discounted;
    total += Math.round(taxed * 100) / 100;
  }
  if (coupon === 'WELCOME' && total > 50) {
    total -= 10;
  } else if (coupon === 'VIP') {
    total *= 0.8;
  }
  if (country === 'NL' || country === 'BE') {
    total += total > 100 ? 0 : 4.95;
  } else if (country === 'DE') {
    total += total > 150 ? 0 : 6.95;
  } else {
    total += 12.5;
  }
  return total;
};
