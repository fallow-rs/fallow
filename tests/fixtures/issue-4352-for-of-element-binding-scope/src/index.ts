import { Table } from "./table.js";

const tables: Table[] = [new Table()];

function all(): Table[] {
  return tables;
}

for (const t of tables) {
  t.setVip();
}

for (const u of all()) {
  u.seat();
}

export function g(t: { lock(): void }): void {
  t.lock();
}

export function h(u: { reset(): void }): void {
  u.reset();
}
