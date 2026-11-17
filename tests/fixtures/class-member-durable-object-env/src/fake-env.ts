export const env = {
  ROOM: {
    getByName: (_name: string) => ({ viaFakeEnv: async () => false }),
  },
};
