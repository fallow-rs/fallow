import { orderTotal } from "./order";

export const cartItems: number[] = [];

export const cartTotal = (): number => orderTotal();
