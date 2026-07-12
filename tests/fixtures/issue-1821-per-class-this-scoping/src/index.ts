import { DepPrivParam } from './dep';
import {
  ConsumerPrivParam,
  ConsumerPubField,
  ConsumerHashA,
  ConsumerHashB,
} from './consumers';

new ConsumerPrivParam(new DepPrivParam()).run();
new ConsumerPubField().run();
new ConsumerHashA().run();
new ConsumerHashB().run();
