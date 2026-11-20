export type Order = { id: string; quantity: number };

export const parseRow = (cells: string[]): Order | null => {
  if (cells.length === 2) {
    const [id, amount] = cells;
    if (id && amount) {
      const quantity = Number(amount);
      if (Number.isInteger(quantity) && quantity > 0) {
        return { id, quantity };
      }
    }
  }
  return null;
};
