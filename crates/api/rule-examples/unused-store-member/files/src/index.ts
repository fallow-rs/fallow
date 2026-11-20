import { useCartStore } from "./stores/cart";

const cart = useCartStore();
cart.addItem(1999);
console.log(cart.total);
