import { Entry, Item, Session, Slot } from './lib';

export class Registry {
  private sessions = new Map<string, Session>();
  private entries: Map<string, Entry> = new Map();

  run(id: string): void {
    this.sessions.get(id)?.used();
    this.entries.get(id)!.used();
  }
}

export function useSlot(slots: Map<string, Slot>): void {
  const slot = slots.get('key');
  slot?.used();
}

export function useItems(items: ReadonlyMap<string, Item>): void {
  for (const item of items.values()) {
    item.used();
  }
}
