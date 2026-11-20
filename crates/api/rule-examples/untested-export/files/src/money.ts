export const roundCents = (amount: number): number => Math.round(amount * 100) / 100;

export const formatPrice = (amount: number): string => `EUR ${amount.toFixed(2)}`;
