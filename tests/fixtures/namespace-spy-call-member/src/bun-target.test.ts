import { spyOn, test } from 'bun:test';
import * as target from './bun-target';

test('spies on one member', () => {
  spyOn(target, 'helper');
});
