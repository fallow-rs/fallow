import type { Row } from "./db";

type Handler = (id: string) => Row;

const app = {
  delete(path: string, handler: Handler): number {
    return path.length + handler.length;
  },
};

const PAGE_SIZE = 10 + 5;

app.delete("/items/:id", () => ({ value: PAGE_SIZE }));

export function pageCount(total: number): number {
  return total > 0 ? Math.ceil(total / PAGE_SIZE) : 0;
}
