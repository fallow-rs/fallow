import * as ns from './c';
class D extends ns.C {}
D.live();
let h = { item: new D() };
const alias = h;
h = { item: { rebindWrong(): void {} } };
alias.item.rebindUsed();
h.item.rebindWrong();
const shared = { item: new D() };
shared.item.beforeMutation();
const sharedAlias = shared;
sharedAlias.item = { mutationWrong(): void {} };
shared.item.mutationWrong();
const parent = { inner: { item: new D() } };
const oldInner = parent.inner;
parent.inner = { item: { ancestorWrong(): void {} } };
oldInner.item.ancestorUsed();
parent.inner.item.ancestorWrong();
