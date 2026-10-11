import { make, makeSync, makeGenerator } from './lib';
import type { Service } from './lib';
export const keep: Service[] = [];
const { direct, renamed: alias } = await make();
direct();
alias();
export async function run(): Promise<void> {
  const { defaulted = () => {}, nested: { run } } = await make();
  defaulted();
  run();
  const { syncAwaited } = await makeSync();
  syncAwaited();
}
const { synchronous } = makeSync();
synchronous();
const { promiseOnly } = make();
promiseOnly();
const { generatorOnly } = await makeGenerator();
generatorOnly();
