import { build } from '$service-workerx';

export const load = () => ({ count: build.length });
