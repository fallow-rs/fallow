import * as ns from './c';
import { C } from './c';
import './duplicates.js';
class Sub extends ns.C {}
class BareSub extends C {}
Sub.live();
interface Options { item: Sub }
const factory = (): Options => ({ item: new Sub() });
let opts: Options = factory();
opts.item.namedBefore();
const saved = opts.item;
const alias = opts;
alias.item = { namedWrong() {} };
opts.item.namedWrong();
saved.namedSaved();
const dynamic: Options = factory();
const key: string = 'item';
dynamic[key] = { namedDynamicWrong() {} };
dynamic.item.namedDynamicWrong();
let values = new Map<string, Sub>();
for (const item of values.values()) item.mapBefore();
const oldMap = values;
values = new Map<string, { mapWrong(): void }>();
for (const item of values.values()) item.mapWrong();
for (const [key, item] of values.entries()) item.mapWrong();
values.forEach(item => item.mapWrong());
values.get('one')!.mapWrong();
for (const item of oldMap.values()) item.mapSaved();
values = new Map<string, Sub>();
for (const item of values.values()) item.mapReplacement();
let holder = { item: new Sub(), inner: { item: new Sub() } };
holder.item = holder.item;
holder.item.propertyOverlap();
holder = holder.inner;
holder.item.rootOverlap();
let scalar = new Sub(); scalar = scalar; scalar.scalarOverlap();
class Writer { item: Sub = new Sub(); read(): void { this.item.thisBefore(); this['item'] = { thisWrong() {} }; this.item.thisWrong(); } }
new Writer().read();
const array: BareSub[] = [new BareSub()];
for (const item of array) item.arrayIterated();
array.forEach(item => item.arrayCallback());
class Holder { item: BareSub = new BareSub(); }
const local = new Holder(); local.item.localProperty();
const read = (holder: Holder): void => holder.item.localParameter(); read(local);
const unknown = [{ unknownWrong() {} }];
for (const item of unknown) item.unknownWrong();
const literal = [new BareSub()];
for (const item of literal) item.literalIterated();
literal.forEach(item => item.literalCallback());
const ref = <T>(value: T): { value: T } => ({ value });
const reactive = ref<BareSub[]>([new BareSub()]);
for (const item of reactive.value) item.reactiveIterated();
reactive.value.forEach(item => item.reactiveCallback());
const arrayReader = (items: BareSub[]): void => {
    for (const item of items) item.parameterIterated();
    items.forEach(item => item.parameterCallback());
};
arrayReader([new BareSub()]);
const objectArray: { items: BareSub[] } = { items: [new BareSub()] };
for (const item of objectArray.items) item.objectIterated();
objectArray.items.forEach(item => item.objectCallback());
const useMemo = <T>(factory: () => T, deps: unknown[]): T => factory();
const memo = useMemo(() => new BareSub(), []); memo.memoUsed();
declare const $derived: { <T>(value: T): T; by<T>(factory: () => T): T };
const derived = $derived(new BareSub()); derived.derivedUsed();
const wrap = <T>(factory: () => T): [T] => [factory()];
const [tuple] = wrap(() => new BareSub()); tuple.tupleUsed();
const boundMap = new Map<string, BareSub>();
const mapItem = boundMap.get('one')!; mapItem.boundMapUsed();
