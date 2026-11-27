import { newService } from "./context";
import { ctx } from "./shared-ctx";

ctx.importOnly();

export const importedName = (): void => {
  const { ctx } = newService("imported");
  console.log(typeof ctx);
};
