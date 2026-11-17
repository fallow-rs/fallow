import { env } from 'cloudflare:workers';

export { RoomC } from './room';

export default {
  async fetch(): Promise<Response> {
    return new Response(String(await env.ROOM.getByName('c').viaModuleEnv()));
  },
};
