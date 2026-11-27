import { newService } from "./context";

interface Other {
  mixedOnly(): void;
}

export const fromFactory = (): void => {
  const { ctx } = newService("mixed");
  console.log(typeof ctx);
};

export const fromParameter = (ctx: Other): void => {
  ctx.mixedOnly();
};
