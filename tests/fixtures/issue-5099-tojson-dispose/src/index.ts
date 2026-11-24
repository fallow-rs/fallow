import { Money } from "./money";
import { Lock, AsyncLock, Handle } from "./lock";

console.log(JSON.stringify(new Money()));
using lock = new Lock();
await using asyncLock = new AsyncLock();
const handle = new Handle();
console.log(lock, asyncLock, handle);
