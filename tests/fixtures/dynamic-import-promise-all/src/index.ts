export async function run(): Promise<void> {
  const [{ usedC }, d] = await Promise.all([import('./c'), import('./d')]);
  usedC();
  d.usedD();
}

export async function runRest(): Promise<unknown[]> {
  const [first, ...rest] = await Promise.all([import('./c'), import('./e')]);
  return [first.usedC, rest];
}

export async function runHole(): Promise<void> {
  const [, d] = await Promise.all([import('./f'), import('./d')]);
  d.usedD();
}
