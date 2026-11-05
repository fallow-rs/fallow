// A knex query builder starts with a call (`db("t")`), so the raw SQL method
// sits on a call-chain receiver. A Prisma client often comes from a call
// (`getPrisma()`) or a class field (`this.prisma`). Each line with a
// `// candidate` marker must give one sql-injection candidate. Each line with
// a `// none` marker must give none.
type Builder = {
  where(column: string, value: unknown): Builder;
  whereRaw(sql: string): Builder;
  havingRaw(sql: string): Builder;
  orderByRaw(sql: string): Builder;
  execute(sql: string): Builder;
};

type Knex = ((table: string) => Builder) & { whereRaw(sql: string): Builder };

type Prisma = {
  $queryRawUnsafe(sql: string): unknown;
  $executeRawUnsafe(sql: string): unknown;
};

export const run = (db: Knex, trx: Knex, clause: string): Builder[] => [
  db("t").whereRaw(clause), // candidate
  db("users").where("id", 1).orderByRaw(clause), // candidate
  trx("t").havingRaw(clause), // candidate
  db("t").whereRaw("id = 1"), // none
  db("t").execute("select " + clause), // none
];

export const runPrisma = (getPrisma: () => Prisma, clause: string): unknown[] => [
  getPrisma().$queryRawUnsafe(clause), // candidate
  getPrisma().$executeRawUnsafe(clause), // candidate
  getPrisma().$queryRawUnsafe("select 1"), // none
];

export class Repository {
  constructor(private readonly prisma: Prisma) {}

  find(clause: string): unknown[] {
    return [
      this.prisma.$queryRawUnsafe(clause), // candidate
      this.prisma.$executeRawUnsafe("delete from t"), // none
    ];
  }
}
