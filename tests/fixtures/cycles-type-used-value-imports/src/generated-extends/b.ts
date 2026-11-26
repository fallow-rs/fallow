import { describe } from './a';
export class Table { name = 'table'; label = (): string => describe({ table: this }); }
