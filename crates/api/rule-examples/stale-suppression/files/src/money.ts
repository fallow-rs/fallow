// Money helpers for the checkout.

// fallow-ignore-next-line unused-export
export const formatPrice = (cents: number): string => `${(cents / 100).toFixed(2)} EUR`;
