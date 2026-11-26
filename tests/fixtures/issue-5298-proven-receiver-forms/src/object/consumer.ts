import { newService } from "./context";
import { describeContext } from "./helper";

export const runObject = (): void => {
  const service = newService("a");
  service.ctx.pathUsed();
  const { ctx } = newService("b");
  ctx.destructuredUsed();
  const { nested } = newService("c");
  nested.inner.nestedUsed();
  describeContext(ctx);
};
