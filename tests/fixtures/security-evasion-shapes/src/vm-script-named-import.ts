import { Script } from "vm";
export const compile = (code: string): Script => new Script(code);
