import { SyntheticSchemaService } from "./service";
import { SyntheticSchemaWriter } from "./schema-writer";

type CreatedDb = {
  name: string;
  writer: SyntheticSchemaWriter;
  close: () => Promise<void>;
};

enum WriteMode {
  Golden = "golden",
  Typed = "typed",
}

export async function generate(mode: WriteMode): Promise<void> {
  // Part 2 (Promise.all inference): `createDb` (declared AFTER this consumer)
  // returns Promise<CreatedDb>, so `dbsFromCreate` is CreatedDb[]. The map
  // callback param `dbMapped` (name distinct from the for-of variable below)
  // credits writeSchemaData / writeGraphDiagram / writeSchemaTyped.
  const dbsFromCreate = await Promise.all(["alpha", "beta"].map(async (schemaName) => createDb(schemaName)));

  if (mode === WriteMode.Golden) {
    await resetSchemas(dbsFromCreate);
  }

  await Promise.all(
    dbsFromCreate.map(async (dbMapped) => {
      try {
        if (mode === WriteMode.Golden) {
          await dbMapped.writer.writeSchemaData("Golden");
          await dbMapped.writer.writeGraphDiagram();
        } else {
          await dbMapped.writer.writeSchemaTyped();
        }
      } finally {
        await dbMapped.close();
      }
    }),
  );
}

async function createDb(name: string): Promise<CreatedDb> {
  const service = new SyntheticSchemaService(name);
  const writer = new SyntheticSchemaWriter(service);

  return {
    name,
    writer,
    close: async () => Promise.resolve(),
  };
}

// Part 1 (array-typed formal parameter): the `dbsForReset: CreatedDb[]` param
// plus the `for...of` loop bind `dbReset` (name distinct from the map callback
// param above), so `dbReset.writer.resetSchema` credits resetSchema.
async function resetSchemas(dbsForReset: CreatedDb[]): Promise<void> {
  for (const dbReset of dbsForReset) {
    await dbReset.writer.resetSchema("Golden");
  }
}
