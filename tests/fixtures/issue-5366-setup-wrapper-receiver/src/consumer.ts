import { newService } from "./context";

const setup = (name?: string) => {
  return newService(name ?? "default");
};

const setupConcise = () => newService("concise");

const setupChained = () => setup("chained");

export const runWrappers = (): void => {
  const { ctx } = setup();
  ctx.blockWrapperUsed();
  const { ctx: concise } = setupConcise();
  concise.conciseWrapperUsed();
  const { ctx: chained } = setupChained();
  chained.chainedWrapperUsed();
  const service = setup("path");
  service.ctx.pathWrapperUsed();
};
