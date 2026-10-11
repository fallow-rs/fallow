import * as ns from './c';
class Sub extends ns.C {}
Sub.live();
const make = (): Sub => new Sub();
const item = make(); item.factoryBound();
make().factoryUnnamed();
const hoistedItem = hoisted(); hoistedItem.hoistedBound();
hoisted().hoistedDirect();
function hoisted(): Sub { return new Sub(); }
const objectMaker = () => ({ item: new Sub() });
const object = objectMaker(); object.item.factoryObject();
const late = make(); const box = { item: late }; box.item.lateBox();
class FactorySub extends ns.C { static create() { return new FactorySub(); } }
const factoryItem = FactorySub.create(); factoryItem.staticFactory();
const read = (item: unknown): void => { if (item instanceof Sub) item.narrowed(); };
read(new Sub());
const direct = new Sub(); const { destructured } = direct; destructured();
const asserted: unknown = new Sub(); (asserted as Sub).asserted();
class Child extends Sub { call(): void { super.superUsed(); } }
new Child().call();
class Holder { item; constructor() { this.item = new Sub(); } call(): void { this.item.fieldUsed(); } }
new Holder().call();
let changing = () => new Sub();
const saved = changing(); changing = () => ({ factoryWrong() {} });
const changed = changing(); saved.beforeFactoryRebind(); changed.factoryWrong();
const staticSaved = FactorySub.create();
FactorySub.create = () => ({ staticWrong() {} });
const staticChanged = FactorySub.create(); staticSaved.beforeStaticRebind(); staticChanged.staticWrong();
const result = objectMaker(); result.item.beforeChildWrite();
const oldChild = result.item; const alias = result; alias.item = { childWrong() {} };
oldChild.savedChild(); result.item.childWrong();
const first = objectMaker(); const second = objectMaker();
first.item = { independentWrong() {} }; first.item.independentWrong(); second.item.independentResult();
let rebound = objectMaker(); const oldResult = rebound;
rebound = { item: { rootWrong() {} } }; oldResult.item.savedResult(); rebound.item.rootWrong();
const replaced = objectMaker(); replaced.item = new Sub(); replaced.item.replacementChild();
const asyncMaker = async () => new Sub();
const asyncRead = async (): Promise<void> => {
    const awaited = await asyncMaker(); awaited.awaitedFactory();
    const promise = asyncMaker(); promise.promiseWrong();
};
asyncRead();
const unsupportedAliasMaker = () => { const returned = new Sub(); return returned; };
const unsupportedAlias = unsupportedAliasMaker(); unsupportedAlias.unsupportedAlias();
objectMaker().item.unsupportedObject();
const structural = (item: { unsupportedStructural(): void }) => item.unsupportedStructural();
structural(new Sub());
