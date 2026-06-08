export const resolvers = {
  Query: {
    user(_parent: unknown, args: { id: string }) {
      eval(args.id);
    },
  },
};
