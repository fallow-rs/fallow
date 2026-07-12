export type ExportFlavor = "Golden" | "Typed";

export class SyntheticSchemaService {
  constructor(private readonly _name: string) {}

  async initialize(): Promise<void> {
    if (!this._name) {
      throw new Error("Missing name");
    }
  }

  async resetSchema(_exportFlavor: ExportFlavor): Promise<void> {
    return Promise.resolve();
  }

  async writeSchemaData(_exportFlavor: ExportFlavor): Promise<void> {
    return Promise.resolve();
  }

  async writeGraphDiagram(): Promise<void> {
    return Promise.resolve();
  }

  async createTypedSchema(): Promise<string> {
    return Promise.resolve("export type SyntheticRow = { id: string };");
  }
}
