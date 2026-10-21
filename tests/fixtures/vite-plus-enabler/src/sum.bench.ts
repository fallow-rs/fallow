import { bench } from 'vite-plus/test';
import { sum } from './index';

bench('sum', () => {
  sum(1, 2);
});
