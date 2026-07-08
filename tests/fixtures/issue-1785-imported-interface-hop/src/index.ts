import { SharedDep } from './dep';
import { RenamedDep } from './dep-renamed';
import { LeafDep } from './leaf';
import { DirectUser } from './user-direct';
import { BarrelUser } from './user-barrel';
import { RenamedUser } from './user-renamed';
import { MultiHopUser } from './user-multihop';

new DirectUser({ c: new SharedDep() }).run();
new BarrelUser({ c: new SharedDep() }).run();
new RenamedUser({ c: new RenamedDep() }).run();
new MultiHopUser({ mid: { leaf: new LeafDep() } }).run();
