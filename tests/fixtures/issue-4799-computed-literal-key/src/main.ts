import { ConstKeyView, FieldView, LocalView, OpenView, ShadowView } from "./views";

export class Canvas {
  private readonly view: FieldView;
  private readonly open: OpenView = new OpenView();
  private readonly shadow: ShadowView = new ShadowView();

  constructor() {
    this.view = new FieldView();
  }

  change(action: "zoomIn" | "zoomOut"): void {
    this.view[action]();
  }

  openChange(action: string): void {
    this.open[action as keyof OpenView]();
  }

  shadowChange(action: "zoomIn"): void {
    const run = (action: keyof ShadowView): void => {
      this.shadow[action]();
    };
    run(action);
  }
}

export const changeLocal = (action: "zoomIn" | "zoomOut"): void => {
  const view = new LocalView();
  view[action]();
};

export const changeConstKey = (): void => {
  const keyView = new ConstKeyView();
  const key = "zoomOut";
  keyView[key]();
};
