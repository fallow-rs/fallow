import { usedHelper, anotherUsedExport, reachableLocalConsumer } from './utils';
import { usedExport } from './expected-unused';
import { something } from './file-level';

console.log(
  usedHelper(),
  usedExport,
  something,
  anotherUsedExport,
  reachableLocalConsumer,
);
