import { test, vi } from 'vitest';
import * as target from './dynamic-target';

const memberName = (): 'helper' => 'helper';

test('spies on a computed member', () => {
  vi.spyOn(target, memberName());
});
