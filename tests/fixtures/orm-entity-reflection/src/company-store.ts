import { DataSource } from 'typeorm';

import { Company } from './company.entity';
import { type Note } from './note.entity';

declare const dataSource: DataSource;
declare const orm: { getRepository<T>(name: string): T[] };

export class CompanyStore {
  companies() {
    return dataSource.getRepository(Company).find();
  }

  notes() {
    return orm.getRepository<Note>('note');
  }

  unusedHelper() {
    return 'never called';
  }
}

export const companies = new CompanyStore().companies();
export const notes = new CompanyStore().notes();
