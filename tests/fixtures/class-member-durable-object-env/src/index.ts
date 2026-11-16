import { env, WorkerEntrypoint } from 'cloudflare:workers';
import { env as fakeEnv } from './fake-env';
import { otherStub, roomStub } from './stubs';

export { Lobby } from './lobby';
export { Other } from './other';
export { Room } from './room';

export class Api extends WorkerEntrypoint<Env> {
  async inline(): Promise<boolean> {
    return this.env.ROOM.getByName('api').viaThisEnv();
  }

  async local(): Promise<boolean> {
    const id = this.env.ROOM.idFromName('api');
    const stub = this.env.ROOM.get(id);
    return stub.viaThisEnvLocal();
  }
}

export async function viaParam(workerEnv: Env): Promise<boolean> {
  const stub = workerEnv.ROOM.getByName('param');
  return stub.viaParam();
}

class Plain {
  constructor(private readonly env: typeof fakeEnv) {}

  call(): Promise<boolean> {
    return this.env.ROOM.getByName('plain').viaPlainThis();
  }
}

export default {
  async fetch(): Promise<Response> {
    const stub = env.ROOM.get(env.ROOM.idFromName('a'));
    const helperStub = roomStub('c');
    const results = [
      await stub.viaEnvImport(),
      await env.ROOM.getByName('b').viaEnvImportInline(),
      await roomStub('d').viaHelper(),
      await helperStub.viaHelperLocal(),
      await otherStub('e').viaOtherHelper(),
      await fakeEnv.ROOM.getByName('f').viaFakeEnv(),
      await new Plain(fakeEnv).call(),
    ];
    return new Response(String(results.every(Boolean)));
  },
};
