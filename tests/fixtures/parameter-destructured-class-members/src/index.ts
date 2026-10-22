import { RestSource, Service } from './service';

const noop = (): void => {};
const service = new Service();

const arrow = ({ viaArrow }: Service): void => viaArrow();
arrow(service);

function declared({ viaFunction }: Service): void {
  viaFunction();
}
declared(service);

class Runner {
  run({ viaMethod }: Service): void {
    viaMethod();
  }
}
new Runner().run(service);

const renamed = ({ viaRenamed: local }: Service): void => local();
renamed(service);

const defaulted = ({ viaDefault = noop }: Service): void => viaDefault();
defaulted(service);

const paramDefault = ({ viaParamDefault }: Service = service): void => viaParamDefault();
paramDefault();

const nested = ({ svc: { viaNested } }: { svc: Service }): void => viaNested();
nested({ svc: service });

const untyped = ({ untypedOnly }: any): void => untypedOnly();
untyped(service);

type Handler = ({ signatureOnly }: Service) => void;
declare function declaredOnly({ signatureOnly }: Service): void;
interface Shape {
  run({ signatureOnly }: Service): void;
}
abstract class AbstractRunner {
  abstract run({ signatureOnly }: Service): void;
}
export type { AbstractRunner, Handler, Shape };
export { declaredOnly };

let viaAssign: () => void;
({ viaAssign } = service);
viaAssign();

let other: () => void;
let viaAssignDefault: () => void;
({ viaAssignRenamed: other, viaAssignDefault = noop } = service);
other();
viaAssignDefault();

const restSource = new RestSource();
const spread = ({ first, ...rest }: RestSource): void => {
  first();
  console.log(rest);
};
spread(restSource);

const holder: { svc: Service } = { svc: service };

let viaNestedLiteral: () => void;
({ svc: { viaNestedLiteral } } = { svc: service });
viaNestedLiteral();

let viaNestedHolder: () => void;
({ svc: { viaNestedHolder } } = holder);
viaNestedHolder();

let viaNestedDefault: () => void;
({ svc: { viaNestedDefault } = service } = {});
viaNestedDefault();

const {
  svc: { viaNestedLiteralBinding },
} = { svc: service };
viaNestedLiteralBinding();

const {
  svc: { viaNestedHolderBinding },
} = holder;
viaNestedHolderBinding();

const { svc: { viaNestedDefaultBinding } = service }: { svc?: Service } = {};
viaNestedDefaultBinding();

const { viaLeafDefault = service.viaLeafDefault }: { viaLeafDefault?: () => void } = {};
viaLeafDefault();

const { missing: { dead } = { dead: noop } } = { svc: service } as {
  svc: Service;
  missing?: { dead: () => void };
};
dead();

let viaNestedLiteralPath: () => void;
({ svc: { viaNestedLiteralPath } } = { svc: holder.svc });
viaNestedLiteralPath();

let viaNestedDefaultPath: () => void;
({ svc: { viaNestedDefaultPath } = holder.svc } = {});
viaNestedDefaultPath();

const services: Service[] = [service];

for (const {
  svc: { viaForOfLiteral },
} of [{ svc: service }]) {
  viaForOfLiteral();
}

for (const { viaForOfTyped } of services) {
  viaForOfTyped();
}

let viaForOfAssign: () => void;
for ({ viaForOfAssign } of [service]) {
  viaForOfAssign();
}

for (const {
  svc: { dead: deadInLoop },
} of [{ other: service }] as { svc: { dead: () => void } }[]) {
  deadInLoop();
}

function paramInitializer({ viaParamInitializer } = service): void {
  viaParamInitializer();
}
paramInitializer();

function nestedParamInitializer({ svc: { viaNestedParamInitializer } } = { svc: service }): void {
  viaNestedParamInitializer();
}
nestedParamInitializer();

const typeofParam = ({ svc: { viaTypeofParam } }: typeof holder): void => viaTypeofParam();
typeofParam(holder);

const typeofNested = ({ svc: { viaTypeofNested } }: { svc: typeof service }): void =>
  viaTypeofNested();
typeofNested({ svc: service });

const registry = { holder };
const typeofQualified = ({ svc: { viaTypeofQualified } }: typeof registry.holder): void =>
  viaTypeofQualified();
typeofQualified(holder);

const {
  svc: { viaGetter },
} = {
  get svc() {
    return service;
  },
};
viaGetter();

const {
  svc: { viaSpread },
} = { ...holder };
viaSpread();

const {
  svc: { viaSpreadLiteral },
} = { ...{ svc: service } };
viaSpreadLiteral();
