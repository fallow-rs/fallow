// A Drizzle-style ORM client. `db.query` is its query-builder accessor.
export const db = {
  query: { orgs: { findFirst: (a: unknown): unknown => a } },
};
