import type TypeOnlyDep = require('type-only-dep');
import type * as TypeOnlyEsm from 'type-only-esm-dep';
import RuntimeDep = require('runtime-dep');

export const readTypeOnly = (value: TypeOnlyDep.Shape): number => value.n;

export const readTypeOnlyEsm = (value: TypeOnlyEsm.Shape): number => value.n;

export const readRuntime = (): number => Object.keys(RuntimeDep).length;
