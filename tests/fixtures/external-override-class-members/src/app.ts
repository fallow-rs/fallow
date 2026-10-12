import { PortContainer, ChildContainer } from "./container";
import { NotFound, Failure, PlainFailure, LooseFailure } from "./errors";
import { LocalChild } from "./local";
import { Mixed, MixedMember, MixedArray } from "./mixed";
import { ProjectLeaf } from "./project-leaf";
import { FromMissing } from "./unresolved";
import { LocalEvent } from "./shadowed-global";
import { RenamedContainer } from "./renamed";
import DefaultContainer from "./default-container";

export const instances: unknown[] = [
  new PortContainer(),
  new ChildContainer(),
  new NotFound(),
  new Failure(),
  new PlainFailure(),
  new LooseFailure(),
  new LocalChild(),
  new Mixed(),
  new MixedMember(),
  new MixedArray(),
  new ProjectLeaf(),
  new FromMissing(),
  new LocalEvent(),
  new RenamedContainer(),
  new DefaultContainer(),
];
