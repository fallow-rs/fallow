import type { Annotated } from "./annotated";
import type { LazyThis } from "./lazy-this";
import type { NotInstance } from "./not-instance";
import type { Nullish } from "./nullish";
import type { Remote } from "./remote";
import type { Unannotated } from "./unannotated";
import type { ArrowReset } from "./arrow-reset";
import type { Local } from "./shared-name";
import type { Logical } from "./logical";
import type { PrivateShadow } from "./private-shadow";
import type { Wrapped } from "./wrapped";
import { arrowReset } from "./arrow-reset";
import { logical } from "./logical";
import { privateShadow } from "./private-shadow";
import { shared, warmUp } from "./shared-name";
import { wrapped } from "./wrapped";
import { annotated } from "./annotated";
import { lazyThis } from "./lazy-this";
import { label, notInstanceClass } from "./not-instance";
import { nullish } from "./nullish";
import { remote, remoteClass, remoteLabel } from "./remote-instance";
import { unannotated } from "./unannotated";

export type Instances = [
  Annotated,
  ArrowReset,
  LazyThis,
  Local,
  Logical,
  NotInstance,
  Nullish,
  PrivateShadow,
  Remote,
  Unannotated,
  Wrapped,
];

annotated.createScene();
unannotated.createScene();
lazyThis.createScene();
nullish.createScene();
remote.createScene();
wrapped.createScene();
logical.createScene();
warmUp();

// These values are not instances, so they must not credit instance members.
(label as any).ping();
(notInstanceClass as any).pong();
(remoteLabel as any).unusedRemote();
(remoteClass as any).unusedRemote();
(privateShadow as any).createScene();
(arrowReset as any).createScene();

// `shared` is also the name of a local `Remote.getInstance()` result in
// `warmUp`, so the export has no single target and credits no class:
// neither `Local.reset` nor `Remote.reset`.
shared.createScene();
shared.reset();
