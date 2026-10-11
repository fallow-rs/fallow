import type { Item } from './barrel';
import { C as ControlC } from './c';
import { Decoy as C } from './other';
function consume(items: ReadonlyArray<Item>, index: number): void { items[index]!.widget.go(); }
consume([{ widget: new ControlC() }], 0);
new ControlC().direct();
export function exerciseUnknown(value: { unknownOnly(): void }): void { value.unknownOnly(); }
new C().direct();
