import { DepIface } from './dep';
import { ConsumerInline } from './consumer-inline';
import { ConsumerCtorNew } from './consumer-ctor-new';
import { ConsumerIface } from './consumer-iface';

new ConsumerInline().run();
new ConsumerCtorNew().run();
new ConsumerIface(new DepIface()).run();
