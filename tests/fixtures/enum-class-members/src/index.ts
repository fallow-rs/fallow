import { Status, MyService, ObjectPropertyService } from './definitions';

console.log(Status.Active);
const svc = new MyService();
svc.greet();

const holder = { property: new ObjectPropertyService() };
console.log(holder.property.usedThroughObject);
