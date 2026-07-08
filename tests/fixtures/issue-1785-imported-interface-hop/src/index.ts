import { SharedDep } from './dep';
import { DirectUser } from './user-direct';
import { BarrelUser } from './user-barrel';

new DirectUser({ c: new SharedDep() }).run();
new BarrelUser({ c: new SharedDep() }).run();
