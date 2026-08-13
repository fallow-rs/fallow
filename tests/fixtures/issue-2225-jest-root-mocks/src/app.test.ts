import { ExtensionStorage } from '@bacons/apple-targets';
import { combine } from './index';

jest.mock('lodash');
jest.mock('@bacons/apple-targets');

describe('app', () => {
  it('uses the root-level manual mocks', () => {
    expect(combine()).toEqual({});
    expect(ExtensionStorage.get()).toBe('mock');
  });
});
