export const useUserStore = defineStore('user', () => ({
  name: 'Ada',
}));

export const unusedStoreHelper = () => null;

export type UseUserStoreType = ReturnType<typeof useUserStore>;
