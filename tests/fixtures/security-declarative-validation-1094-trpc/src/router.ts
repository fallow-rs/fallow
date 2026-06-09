import { initTRPC } from "@trpc/server";

declare const schema: unknown;

const t = initTRPC.create();

export const router = t.router({
  user: t.procedure.input(schema).query(({ input }) => {
    eval(input.id);
  }),
});
