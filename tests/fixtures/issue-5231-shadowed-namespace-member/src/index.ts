import * as e from "./errors";

export const run = (): unknown => {
  try {
    return e.usedError();
  } catch (e: any) {
    return e.message;
  }
};
