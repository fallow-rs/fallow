import { C } from './c';
class Other {go() {}}
const consume = (items: [{widget:C},{widget:Other}]): void => {items[1].widget.go();};consume([{widget:new C()},{widget:new Other()}]);

import { C as ControlC } from './c';
new ControlC().direct();
export const exerciseUnknown = (value: { unknownOnly(): void }): void => { value.unknownOnly(); };
