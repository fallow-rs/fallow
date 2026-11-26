import { newService } from "./context";

interface Other {
  collisionOnly(): void;
}

export const readFirst = (): void => {
  const { ctx } = newService("d");
  ctx.pathUsed();
};

export const readOther = (ctx: Other): void => {
  ctx.collisionOnly();
};

export const readDefault = (): void => {
  const { ctx: withDefault = { defaultOnly: (): void => {} } } = newService("e");
  withDefault.defaultOnly();
};
