export { Room } from './room';
export { queueHandler } from './queue';

export default {
  async fetch(request, env, ctx): Promise<Response> {
    const stub = env.ROOM.get(env.ROOM.idFromName('a'));
    return new Response(String(await stub.viaFetchEnv()));
  },
} satisfies ExportedHandler<Env>;
