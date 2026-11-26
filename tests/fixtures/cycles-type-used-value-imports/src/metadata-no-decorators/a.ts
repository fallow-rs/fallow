import { Table } from './b';
export type Row = { table: Table };
export const describe = (row: Row): string => row.table.name;
