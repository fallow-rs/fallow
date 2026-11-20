import { cartItems } from "./cart";

export const orderTotal = (): number =>
  cartItems.reduce((sum, price) => sum + price, 0);
