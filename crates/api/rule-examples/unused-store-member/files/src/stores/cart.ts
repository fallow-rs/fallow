import { defineStore } from "pinia";

export const useCartStore = defineStore("cart", {
  state: () => ({ items: [] as number[] }),
  getters: {
    total: (state) => state.items.reduce((sum, price) => sum + price, 0),
    discountTotal: (state) => state.items.length * 100,
  },
  actions: {
    addItem(price: number) {
      this.items.push(price);
    },
  },
});
