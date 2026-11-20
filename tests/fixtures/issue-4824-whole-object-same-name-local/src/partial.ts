import { C, D } from "./views";

export const three = (key: string): void => { const view = new C(); view[key as keyof C](); };
export const four = (): void => { const view = new D(); view.a(); };
