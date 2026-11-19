export interface Backend {
  isLive(): boolean;
  readText(name: string): string;
}

export class Loader {
  private _backend: Backend | undefined;

  load(backend: Backend): string {
    this._backend = backend;
    if (!backend.isLive()) return "";
    return this._backend.readText("trace.json");
  }
}
