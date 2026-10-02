import { resolve } from 'node:path';

export const toolPath = resolve('node_modules/.bin/cli-tool');
export const runnerPath = `./node_modules/.bin/runner`;
export const missingPath = resolve('node_modules/.bin/not-declared');
