import fs from "node:fs";

import { ExportFlavor, SyntheticSchemaService } from "./service";

export class SyntheticSchemaWriter {
  constructor(private readonly _schemaService: SyntheticSchemaService) {}

  async resetSchema(exportFlavor: ExportFlavor): Promise<void> {
    if (exportFlavor === "Golden") {
      await this._schemaService.initialize();
    }
    await this._schemaService.resetSchema(exportFlavor);
  }

  async writeSchemaData(exportFlavor: ExportFlavor): Promise<void> {
    if (exportFlavor === "Golden") {
      await this._schemaService.initialize();
    }
    await this._schemaService.writeSchemaData(exportFlavor);
  }

  async writeGraphDiagram(): Promise<void> {
    await this._schemaService.initialize();
    await this._schemaService.writeGraphDiagram();
  }

  async writeSchemaTyped(): Promise<void> {
    await this._schemaService.initialize();
    const typeDeclarations = await this._schemaService.createTypedSchema();
    await fs.promises.writeFile("./synthetic-typed-schema.ts", typeDeclarations, { encoding: "utf-8" });
  }

  // FN guard: never called anywhere. Must still report unused-class-member.
  async deadMethod(): Promise<void> {
    return Promise.resolve();
  }
}
