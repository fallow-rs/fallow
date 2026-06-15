import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async () => {
  return {
    serverKey: 'consumed by the universal load below, not by +page.svelte directly',
  };
};
