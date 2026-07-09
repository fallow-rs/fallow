import { ImportedDep } from './dep';
import { makeUser } from './user';

makeUser(new ImportedDep()).run();
