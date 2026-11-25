import * as e from "./errors";

export const run = (): number => {
  try {
    return e.usedError();
  } catch (e) {
    return String(e).length;
  }
};
