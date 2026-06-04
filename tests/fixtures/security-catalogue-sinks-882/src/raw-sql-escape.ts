// Positive: raw SQL escape hatches accept non-literal fragments.
interface PrismaLike {
  $queryRawUnsafe(sql: string): unknown;
}

interface KnexLike {
  whereRaw(sql: string): unknown;
}

export function query(prisma: PrismaLike, knex: KnexLike, clause: string): unknown[] {
  return [prisma.$queryRawUnsafe(clause), knex.whereRaw(clause)];
}
