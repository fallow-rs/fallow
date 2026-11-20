export interface Box<T> {
  value: T;
}

export interface Entry {
  label: string;
}

export interface Item {
  id: number;
}

export interface Unrelated {
  note: string;
}

export interface Detached {
  flag: boolean;
}

const entryBox: Box<Entry> = { value: { label: "a" } };

const items: Item[] = [{ id: 1 }];

const nested = { items };

// The annotation of `detached` does not reach an export, because the
// initializer of `local` reads a property of it.
const detached: Detached = { flag: true };

const local = detached.flag;

export const status = { latency: entryBox, nested: [nested], local };
