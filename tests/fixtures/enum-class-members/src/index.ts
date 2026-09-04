import { Status, MyService, ObjectPropertyService } from './definitions';
import * as Definitions from './definitions';
import { services } from './container-barrel';

console.log(Status.Active);
const svc = new MyService();
svc.greet();

const holder = { property: new ObjectPropertyService() };
console.log(holder.property.usedThroughObject);

const propertyAlias = holder.property;
console.log(propertyAlias.usedThroughPropertyAlias);

const holderAlias = holder;
console.log(holderAlias.property.usedThroughObjectAlias);

const { property: destructuredProperty } = holder;
console.log(destructuredProperty.usedThroughDestructure);

console.log(holder['property'].usedThroughComputed);

const dynamicProperty = 'property';
console.log(holder[dynamicProperty].usedThroughDynamicKey);

const qualifiedHolder = { property: new Definitions.ObjectPropertyService() };
console.log(qualifiedHolder.property.usedThroughQualifiedConstructor);

console.log(services.property.usedThroughCrossModuleContainer);
