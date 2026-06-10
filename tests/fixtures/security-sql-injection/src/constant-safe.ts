// Negative (constant): a numeric sentinel coerced through String() is static,
// so sql.raw(...) must not be treated as attacker-controlled input.
import { sql } from "drizzle-orm";

const MISSING_LINE_NUMBER_SENTINEL = -1;

export function missingLineSentinel(): unknown {
  return sql.raw(String(MISSING_LINE_NUMBER_SENTINEL));
}
