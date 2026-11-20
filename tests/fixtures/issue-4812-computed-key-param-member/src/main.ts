import { LiteralView, RedeclaredKeyView, ShadowView, StringView, UnionView } from "./views";

export const changeUnion = (view: UnionView, action: "zoomIn" | "zoomOut"): void => {
  view[action]();
};

export const changeString = (view: StringView, action: string): void => {
  view[action as keyof StringView]();
};

export const changeLiteral = (view: LiteralView): void => {
  view["zoomIn"]();
};

export const changeShadowed = (view: ShadowView, key: string): void => {
  const run = (view: Record<string, () => void>): void => {
    view[key]();
  };
  run({});
  view.zoomIn();
};

export const changeRedeclaredKey = (view: RedeclaredKeyView, action: "zoomIn"): void => {
  const run = (action: string): void => {
    view[action as keyof RedeclaredKeyView]();
  };
  run(action);
};
