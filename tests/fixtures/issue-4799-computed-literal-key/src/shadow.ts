import { importedKey } from "./keys";
import { CatchView, CondView, ImportView, LaterConstView, NamespaceView, VarView } from "./views";

export const changeCond = (forward: boolean): void => {
  const condView = new CondView();
  condView[forward ? "zoomIn" : "zoomOut"]();
};

const catchKey: "a" | "b" = "a";
export const catchCase = (): void => {
  const catchView = new CatchView();
  try {
    catchView.a();
  } catch (catchKey: any) {
    catchView[catchKey]();
  }
};

export function varCase(input: string, cond: boolean, varKey: "a" | "b"): void {
  const varView = new VarView();
  if (cond) {
    var varKey: any = input;
  }
  varView[varKey]();
}

export function laterConstCase(laterKey: "a" | "b", input: keyof LaterConstView): void {
  const laterView = new LaterConstView();
  const run = (): void => laterView[laterKey]();
  {
    const laterKey = input;
    void laterKey;
  }
  run();
}

const nsKey: "a" | "b" = "a";
const nsView = new NamespaceView();
export namespace Runner {
  export function run(nsKey: keyof NamespaceView): void {
    nsView[nsKey]();
  }
}

export function importCase(): void {
  class Holder {
    static {
      const importedKey: "a" | "b" = "a";
      void importedKey;
    }
  }
  void Holder;
  const importView = new ImportView();
  importView[importedKey]();
}
