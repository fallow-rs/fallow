export const parseAmount = (raw: string): number => Math.round(Number.parseFloat(raw) * 100);
export const formatAmount = (cents: number): string => (cents / 100).toFixed(2);
