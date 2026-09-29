type Line = { price: number; quantity: number; discount?: number; taxRate?: number };

export const invoiceTotal = (lines: Line[]): number => {
  let total = 0;
  for (const line of lines) {
    const gross = line.price * line.quantity;
    const discounted = line.discount ? gross - gross * line.discount : gross;
    const taxed = line.taxRate ? discounted + discounted * line.taxRate : discounted;
    total += Math.round(taxed * 100) / 100;
  }
  return total;
};
