import { CyclicUser } from './user';
import { LeafDep } from './leaf';

new CyclicUser({ b: { a: { leaf: new LeafDep() } } }).run();
