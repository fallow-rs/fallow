import { describe, expect, it, vi } from 'vitest';
import { render } from '@/index';
import { sibling } from '../utils/sibling';

// Issue #378: alias-resolved vi.mock() target with NO __mocks__/ sibling on
// disk. Vitest auto-mocks in-memory, so the synthesised
// `@/utils/__mocks__/exportElementAsPng` import must NOT surface as
// `unresolved-import`.
vi.mock('@/utils/exportElementAsPng');

// Same scenario via a relative specifier, exercising the rsplit branch in
// `vitest_auto_mock_source`. Also no `__mocks__/` sibling on disk.
vi.mock('../utils/sibling');

describe('render', () => {
  it('uses the auto mock', () => {
    expect(render()).toBeDefined();
    expect(sibling()).toBeDefined();
  });
});
