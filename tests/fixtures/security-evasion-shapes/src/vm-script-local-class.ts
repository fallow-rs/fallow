class Script {
  constructor(readonly source: string) {}
}
export const compile = (code: string): Script => new Script(code);
