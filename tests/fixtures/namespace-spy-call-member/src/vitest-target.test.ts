import { test, vi } from 'vitest';
import * as target from './vitest-target';

test('spies on one member', () => {
  vi.spyOn(target, 'helper');
});
