import type { PageLoad } from './$types';

export const load: PageLoad = async () => {
  return {
    used: 'rendered',
    dead: 'nobody reads this',
  };
};
