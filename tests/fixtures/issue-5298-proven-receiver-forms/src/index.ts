import { TitleManager } from "./branch/consumer";
import { chained, created } from "./fluent/consumer";
import { readGuards } from "./fluent/guards";
import { readDefault, readFirst, readOther } from "./object/collision";
import { runObject } from "./object/consumer";
import { runLeaf } from "./subclass/consumer";

new TitleManager().update("project");
runLeaf();
runObject();
readGuards();
readFirst();
readDefault();
readOther({ collisionOnly: (): void => {} });
export { chained, created };
