// Positive: an XPath expression built from non-literal input is an
// xpath-injection candidate (CWE-643).
import * as xpath from "xpath";

export function lookup(doc: unknown, userExpr: string): unknown {
  return xpath.select(userExpr, doc);
}
