import { DurableObject } from 'cloudflare:workers';

export class RoomB extends DurableObject<Env> {
  async viaB(): Promise<boolean> {
    return true;
  }

  // Only package a calls this name, on a stub of its own `RoomA`.
  async viaA(): Promise<boolean> {
    return true;
  }
}
