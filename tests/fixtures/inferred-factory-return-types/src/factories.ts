export type Answers = Record<string, number>;
export type Snapshot = { size: number };
export type Settings = { mode: string };
export type Scratch = { note: string };

export function makeCache() {
  function get(): Answers | undefined {
    return undefined;
  }
  return { get };
}

export const makeReader = () => ({
  read: (): Snapshot => ({ size: 1 }),
});

export function makeSettings() {
  return { mode: "fast" } as Settings;
}

export function makeCounter() {
  const scratch: Scratch = { note: "local" };
  const count = () => scratch.note.length;
  return { count };
}
