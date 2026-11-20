const addItem = (cart: string[], sku: string): string[] => [...cart, sku];

export const cart = addItem([], "shirt");
