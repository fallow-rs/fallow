export const formatPrice = (cents: number): string => `${roundCents(cents) / 100} EUR`;

export const roundCents = (cents: number): number => Math.round(cents);

export const legacyFormat = (cents: number): string => `EUR ${cents / 100}`;
