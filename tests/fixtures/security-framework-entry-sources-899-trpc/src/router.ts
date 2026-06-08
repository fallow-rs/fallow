declare const schema: unknown;
declare const t: {
  router(routes: unknown): unknown;
  procedure: {
    input(inputSchema: unknown): {
      query(handler: (opts: { input: { id: string } }) => unknown): unknown;
    };
  };
};

export const router = t.router({
  user: t.procedure.input(schema).query(({ input }) => {
    eval(input.id);
  }),
});
