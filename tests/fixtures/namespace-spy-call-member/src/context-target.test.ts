import { test } from 'node:test';
import * as target from './context-target';

test('spies on one member', (t) => {
  t.mock.method(target, 'helper');
});
