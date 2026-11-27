import { newService } from "./context";

const setup = () => newService("repeated");

export const firstTest = (): void => {
  const { sut, ctx } = setup();
  ctx.repeatedUsed();
  console.log(sut);
};

export const secondTest = (): void => {
  const { ctx } = setup();
  ctx.repeatedUsed();
};

export const directTests = (): void => {
  {
    const { ctx: direct } = newService("a");
    direct.repeatedDirectUsed();
  }
  {
    const { ctx: direct } = newService("b");
    direct.repeatedDirectUsed();
  }
};
