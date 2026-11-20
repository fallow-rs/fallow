export const formatTotal = (cents: number): string => `${(cents / 100).toFixed(2)} EUR`;

export const formatPrice = (cents: number): string => `EUR ${cents / 100}`;
