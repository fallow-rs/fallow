import { newService } from "./context";

class OtherContext {
  shadowedOnly(): void {}
}

const viaParam = (newService: () => { ctx: OtherContext }) => {
  return newService();
};

export const runGuards = (): string => {
  const { ctx } = viaParam(() => ({ ctx: new OtherContext() }));
  ctx.shadowedOnly();
  return typeof newService;
};
