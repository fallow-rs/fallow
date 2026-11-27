import { Table } from './b';
const column = (_target: object, _key: string): void => {};
export class Entity {
  @column tables?: Promise<Table>;
  use(table: Table): Table { return table; }
}
