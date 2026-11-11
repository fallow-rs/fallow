import type { DailyQuota } from './quota';
import { Repo } from './repo';

export { DailyQuota } from './quota';

interface Env {
  QUOTA: DurableObjectNamespace<DailyQuota>;
  REPO: Repo;
}

export default {
  async fetch(_request: Request, env: Env): Promise<Response> {
    const stub = env.QUOTA.get(env.QUOTA.idFromName('daily'));
    const byName = env.QUOTA.getByName('daily');
    const id = env.QUOTA.idFromName('local');
    const viaId = env.QUOTA.get(id);
    const annotated: DurableObjectStub<DailyQuota> = env.QUOTA.get(
      env.QUOTA.idFromName('annotated'),
    );
    const item = env.REPO.get('key');
    const results = [
      await stub.viaGet(1),
      await byName.viaGetByName(2),
      await viaId.viaIdLocal(3),
      await annotated.viaAnnotatedStub(3),
      item.unusedOnRepo(),
    ];
    return new Response(String(results.every(Boolean)));
  },
};
