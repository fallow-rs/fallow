import { A, B } from "./views";

export const one = (key: string): void => { const view = new A(); view[key as keyof A](); };
export const two = (key: string): void => { const view = new B(); view[key as keyof B](); };
