import type { PageLoad } from './$types';

export const load: PageLoad = async () => {
  return {
    globalKey: 'read via page.data in a shared component',
  };
};
