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
