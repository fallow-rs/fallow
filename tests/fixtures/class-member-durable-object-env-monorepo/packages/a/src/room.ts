import { DurableObject } from 'cloudflare:workers';

export class RoomA extends DurableObject<Env> {
  async viaA(): Promise<boolean> {
    return true;
  }

  // Only package b calls this name, on a stub of its own `RoomB`.
  async viaB(): Promise<boolean> {
    return true;
  }
}

export class LobbyA extends DurableObject<Env> {
  async viaLobby(): Promise<boolean> {
    return true;
  }
}
