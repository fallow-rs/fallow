export function format(value: number, compact: boolean, locale?: string): string {
  if (compact) return String(value);
  return value.toLocaleString(locale);
}
