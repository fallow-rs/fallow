export const addColumn = async (table: string, column: string): Promise<string> =>
  `${table}.${column}`;
