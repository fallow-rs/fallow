// Same rule as an env schema that skips validation only under NODE_ENV=test.
const expected = process.env.EXPECTED_NODE_ENV ?? "test";
if (process.env.NODE_ENV !== expected) {
  throw new Error(`Invalid environment variables: NODE_ENV=${process.env.NODE_ENV}`);
}
export const databaseUrl = "file:test.db";
