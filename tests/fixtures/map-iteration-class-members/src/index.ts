import { Item } from './lib';
const items = new Map<string, Item>();
for (const [key, value] of items.entries()) {
  value.entriesValue();
  key.keyOnly();
}
for (const [, value] of items.entries()) value.omittedKey();
items.forEach((value, key, map) => {
  value.arrowValue();
  key.keyOnly();
  map.keyOnly();
});
items.forEach(function (value, key) { value.functionValue(); key.keyOnly(); });
items.forEach((other, value) => value.keyOnly());
for (const [value] of items.entries()) value.keyOnly();
export class Registry {
  private items: ReadonlyMap<string, Item> = new Map();
  run(): void { this.items.forEach(value => value.fieldValue()); }
}
export function use(items: ReadonlyMap<string, Item>): void {
  for (const [, value] of items.entries()) value.scopedValue();
}
export function unknown(items: unknown): void {
  items.forEach(value => value.unknownOnly());
  for (const [, value] of items.entries()) value.unknownOnly();
}
items.map(value => value.unsupportedOnly());

export function useVar(items: ReadonlyMap<string, Item>): void {
  for (var [, carried] of items.entries()) {}
  carried?.varValue();
}

export function unknownLocal(): void {
  let items: unknown;
  items.forEach(value => value.unknownOnly());
}
export function unknownDestructure(source: unknown): void {
  const { items } = source;
  items.forEach(value => value.unknownOnly());
}

const outer = new Map<string, Item>();
for (const [, outer] of items.entries()) {}
outer.forEach(value => value.afterLoop());

export function unknownRest(...items): void {
  items.forEach(value => value.restOnly());
}
export function unknownCatch(): void {
  try { throw new Error(); } catch (items) {
    items.forEach(value => value.catchOnly());
  }
}

for (const items in { first: true }) {
  items.forEach(value => value.forInOnly());
}

export namespace Consumers {
  export function unknown(...items): void {
    items.forEach(value => value.namespaceRestOnly());
  }
}
