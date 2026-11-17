import { DurableObject } from 'cloudflare:workers';
import type { Other } from './other';
import type { Room } from './room';

export class Lobby extends DurableObject<Env> {
  private room: DurableObjectStub<Room>;
  private other: DurableObjectStub<Other>;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.room = env.ROOM.getByName('lobby');
    this.other = env.OTHER.getByName('lobby');
  }

  async ping(): Promise<boolean> {
    const results = [await this.room.viaField(), await this.other.viaOtherField()];
    return results.every(Boolean);
  }
}
