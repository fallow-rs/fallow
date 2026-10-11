import { C } from './c';
class Sub extends C {}
const holder = { item: new Sub(), item: null };
holder.item?.duplicateWrong();
const valid = { item: null, item: new Sub() };
valid.item.duplicateReplacement();
