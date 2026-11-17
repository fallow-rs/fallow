import { env } from 'cloudflare:workers';
import type { Other } from './other';
import type { Room } from './room';

export function roomStub(name: string): DurableObjectStub<Room> {
  return env.ROOM.getByName(name);
}

export const otherStub = (name: string): DurableObjectStub<Other> =>
  env.OTHER.getByName(name);
