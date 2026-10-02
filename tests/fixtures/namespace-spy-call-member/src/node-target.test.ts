import { mock, test } from 'node:test';
import * as target from './node-target';

test('spies on one member', () => {
  mock.method(target, 'helper');
});
