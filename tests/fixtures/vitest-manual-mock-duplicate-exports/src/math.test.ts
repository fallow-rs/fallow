import { describe, expect, it, vi } from 'vitest';
import { formatAmount as legacyFormat } from './legacy/format';
import { formatAmount, parseAmount } from './lib/parse';
import { now as fakeNow } from './shared/__mocks__/time';
import { now } from './time';

vi.mock('./lib/parse');

describe('math', () => {
  it('uses the manual mock', () => {
    expect(parseAmount('12')).toBe(2);
    expect(formatAmount(5)).toBe(legacyFormat(5));
    expect(now()).toBeGreaterThan(fakeNow());
  });
});
