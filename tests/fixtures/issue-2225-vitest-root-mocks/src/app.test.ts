import { describe, expect, it, vi } from 'vitest';
import { ExtensionStorage } from '@bacons/apple-targets';
import { combine } from './index';

vi.mock('lodash');
vi.mock('@bacons/apple-targets');

describe('app', () => {
  it('uses the root-level manual mocks', () => {
    expect(combine()).toEqual({});
    expect(ExtensionStorage.get()).toBe('mock');
  });
});
