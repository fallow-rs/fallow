// An Express project with a storage client that has a `download` method.
interface Storage {
  download(key: string): Promise<Uint8Array>;
}

export async function fetchObject(storage: Storage, key: string): Promise<Uint8Array> {
  return storage.download(key);
}
