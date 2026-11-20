const addItem = (cart: string[], sku: string): string[] => [...cart, sku];

// @ts-expect-error
export const cart = addItem([], 42);
