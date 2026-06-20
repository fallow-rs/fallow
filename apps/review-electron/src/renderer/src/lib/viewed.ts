/** Per-file reviewed state, persisted in a key-value store (localStorage). */
export type KeyValueStore = Pick<Storage, "getItem" | "setItem">;

const key = (path: string): string => `fallow-viewed:${path}`;

export const isViewed = (store: KeyValueStore, path: string): boolean =>
  store.getItem(key(path)) === "1";

export const setViewed = (store: KeyValueStore, path: string, viewed: boolean): void => {
  store.setItem(key(path), viewed ? "1" : "0");
};
