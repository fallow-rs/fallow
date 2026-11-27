import { Remote } from "./remote";

export class Local {
  private static instance: Local;
  createScene(): void {}
  reset(): void {}
  static getInstance(): Local {
    if (!Local.instance) {
      Local.instance = new Local();
    }
    return Local.instance;
  }
}

export const warmUp = (): void => {
  const shared = Remote.getInstance();
  void shared;
};

export const shared = Local.getInstance();
