export class Service {
  direct(): void {}
  renamed(): void {}
  defaulted(): void {}
  nested = { run(): void {} };
  synchronous(): void {}
  syncAwaited(): void {}
  promiseOnly(): void {}
  generatorOnly(): void {}
  never(): void {}
}
export async function make(): Promise<Service> { return new Service(); }
export function makeSync(): Service { return new Service(); }
export function* makeGenerator() { return new Service(); }
