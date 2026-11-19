export interface PersistenceAdapter {
  load(): Promise<string>;
  save(data: string): Promise<void>;
}

export const run = async (adapter: PersistenceAdapter): Promise<void> => {
  await adapter.save(await adapter.load());
};

export const configure = (options: { adapter: { load(): string } }): string =>
  options.adapter.load();
