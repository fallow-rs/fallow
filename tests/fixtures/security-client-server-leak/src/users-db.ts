// Server-side data access that reads a non-public env secret. Reached from
// "use client" files only through Server Action modules (issue #2941).
export function findUser(id: string): string {
  return `${process.env.USERS_DB_URL}/${id}`;
}
