import { fork } from 'node:child_process';
import React from 'react';
import debounce from 'lodash/debounce';
import type { Shape } from './types';
import { renderHeavy } from './heavy/view';
import { shared } from './shared';
import './styles.css';
import { declared } from './decl';

export * from './reexported';

const legacy = require('./legacy');

export const loadLazy = () => import('./lazy');
export const loadPage = (name: string) => import(`./pages/${name}.ts`);
export const eagerModules = import.meta.glob('./eager/*.ts', { eager: true });
export const lazyModules = import.meta.glob('./lazy-glob/*.ts');
export const loadChart = () => import('chart-lib');

export const worker = new Worker(new URL('./worker.ts', import.meta.url));
export const child = fork('./child.js');

export const shape: Shape | null = null;
export const main = (): unknown => [React, debounce, renderHeavy(), shared, legacy, declared];
