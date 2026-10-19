import { PrismaClient } from "@prisma/client";

const prisma = new PrismaClient();

export async function runAll(ops: never[]): Promise<boolean> {
  if (ops.length === 0) return false;
  await prisma.$transaction(ops);
  return true;
}

export async function clearAll(force: boolean): Promise<boolean> {
  if (!force) return false;
  await prisma.$executeRaw`DELETE FROM records`;
  return true;
}
