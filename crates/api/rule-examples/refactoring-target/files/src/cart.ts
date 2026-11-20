type Item = { price: number; quantity: number };

export const subtotal = (items: Item[]): number =>
  items.reduce((sum, item) => sum + item.price * item.quantity, 0);

export const itemCount = (items: Item[]): number =>
  items.reduce((sum, item) => sum + item.quantity, 0);

export const isEmpty = (items: Item[]): boolean => items.length === 0;
