import { describe, expect, it, vi } from 'vitest';

describe('api', () => {
  it('uses the manual mock for imports after the call', async () => {
    vi.doMock('./services/api');
    const { fetchUser } = await import('./services/api');
    expect(fetchUser()).toBe('mock-user');
  });
});
