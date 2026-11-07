import { Table } from "./table.js";
import "./shadow.js";

for (const table of all()) {
  table.setVip();
}

all().forEach((table) => table.lock());

all()[0].seat();

const tables = all();
for (const table of tables) {
  table.clean();
}

function all(): Table[] {
  return [new Table()];
}
