import { C } from './c';
const earlier = (): void => { const item = new Sub(); item.beforeDeclaration(); };
class Sub extends C {}
earlier();
const nested = (opts: { item: Sub }): void => { opts.item.nested(); }; nested({ item: new Sub() });
const destructured = ({ item }: { item: Sub }): void => { item.destructured(); }; destructured({ item: new Sub() });
const factory = (): { item: Sub } => ({ item: new Sub() });
const annotated: { item: Sub } = factory(); annotated.item.annotated();
class Holder { item: Sub = new Sub(); read(): void { this.item.field(); } }
new Holder().read();
const values = new Map<string, Sub>();
for (const item of values.values()) item.mapped();
