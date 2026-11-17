export const queueHandler: ExportedHandler<Env> = {
  queue: async (batch, env) => {
    await env.ROOM.getByName('q').viaQueueEnv();
  },
};

export const plainHandler = {
  async fetch(request, env): Promise<Response> {
    return new Response(String(await env.ROOM.getByName('p').viaPlainObject()));
  },
};
