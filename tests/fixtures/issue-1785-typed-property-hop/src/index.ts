import { OptDep } from './dep';
import { AliasDep } from './alias-dep';
import { UserInterface } from './user-interface';
import { UserAlias } from './user-alias';
import { SameFileDep, SameFileUser } from './samefile';

new UserInterface({ c: new OptDep() }).run();
new UserAlias({ c: new AliasDep() }).run();
new SameFileUser({ c: new SameFileDep() }).run();
