import { test } from 'vitest';
import * as target from './local-target';

const spyOn = (object: object, key: string): unknown => Reflect.get(object, key);

test('passes the namespace to a local helper', () => {
  spyOn(target, 'helper');
});
