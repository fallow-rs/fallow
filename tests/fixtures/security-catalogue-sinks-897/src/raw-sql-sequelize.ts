// Positive: Sequelize.literal(x) is a raw SQL escape hatch (CWE-89). The shipped
// sql-injection raw row now covers the capital-S static form too.
import { Sequelize } from "sequelize";

export function rawOrder(column: string): unknown {
  return Sequelize.literal(column);
}
