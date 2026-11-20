export function formatMoney(cents: number): string {
  return `${(cents / 100).toFixed(2)} EUR`;
}

/** @deprecated Use formatMoney. */
export function formatPrice(cents: number): string {
  return formatMoney(cents);
}
