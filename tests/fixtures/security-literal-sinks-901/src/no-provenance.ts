function chmodSync(_file: string, _mode: number): void {}
function writeFileSync(_file: string, _body: string): void {}
function createConnection(_options: unknown): void {}

class BrowserWindow {
  constructor(_options: unknown) {}
}

export function localLookalikes(): void {
  chmodSync("file", 0o777);
  writeFileSync("/tmp/fallow-token", "secret");
  new BrowserWindow({ webPreferences: { nodeIntegration: true } });
  createConnection({ multipleStatements: true });
}
