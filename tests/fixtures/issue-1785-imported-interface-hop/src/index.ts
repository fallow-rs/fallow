import { SharedDep } from './dep';
import { RenamedDep } from './dep-renamed';
import { DirectUser } from './user-direct';
import { BarrelUser } from './user-barrel';
import { RenamedUser } from './user-renamed';

new DirectUser({ c: new SharedDep() }).run();
new BarrelUser({ c: new SharedDep() }).run();
new RenamedUser({ c: new RenamedDep() }).run();
