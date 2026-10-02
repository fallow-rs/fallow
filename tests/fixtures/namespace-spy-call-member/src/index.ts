import { run as bunRun } from './bun-target';
import { run as contextRun } from './context-target';
import { run as dynamicRun } from './dynamic-target';
import { run as globalRun } from './global-target';
import { run as jestRun } from './jest-target';
import { run as localRun } from './local-target';
import { run as nodeRun } from './node-target';
import { run as vitestRun } from './vitest-target';

bunRun();
contextRun();
dynamicRun();
globalRun();
jestRun();
localRun();
nodeRun();
vitestRun();
