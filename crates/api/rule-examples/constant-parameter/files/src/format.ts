export function format(value: number, compact: boolean, locale?: string): string {
  const amount = value / 100;
  if (compact) {
    return `${Math.round(amount)} EUR`;
  }
  return amount.toLocaleString(locale ?? "en-GB", { style: "currency", currency: "EUR" });
}
