import { promises as fs } from 'node:fs';
import path, { join } from 'node:path';
import { FileMigrationProvider } from 'kysely';

export const migrator = () =>
  new FileMigrationProvider({
    fs,
    path,
    migrationFolder: join(import.meta.dirname, '..', 'schema/migrations'),
  });
