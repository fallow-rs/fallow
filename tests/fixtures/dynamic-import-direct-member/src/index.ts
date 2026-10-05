export async function run(): Promise<void> {
  await (await import('./a')).usedA();
  new (await import('./b')).KB();
}
