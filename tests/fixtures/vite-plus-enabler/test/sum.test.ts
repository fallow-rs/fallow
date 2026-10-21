import { expect, test } from 'vite-plus/test';
import { sum } from '../src/index';

test('sum', () => {
  expect(sum(1, 2)).toBe(3);
});
