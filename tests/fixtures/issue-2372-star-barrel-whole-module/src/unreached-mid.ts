// A hop no entry point reaches, sitting between the unreachable shim and the
// modules the entry point imports directly. Both chain forms leave from here.
export * from './unreached-reentry';
export * as unreachedNs from './unreached-ns-reentry';

export const unreachedMidOne = 1;
