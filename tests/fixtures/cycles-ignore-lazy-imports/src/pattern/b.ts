export async function patternB(suffix: string) {
  const mod = await import(`./a${suffix}`);
  return mod.patternHelper();
}
