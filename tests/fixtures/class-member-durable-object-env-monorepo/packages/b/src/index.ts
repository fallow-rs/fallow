import { env } from 'cloudflare:workers';

export { RoomB } from './room';

export default {
  async fetch(): Promise<Response> {
    return new Response(String(await env.ROOM.getByName('b').viaB()));
  },
};
