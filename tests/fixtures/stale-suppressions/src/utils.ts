// STALE: this export IS used by index.ts, so the suppression has no effect
// fallow-ignore-next-line unused-export
export const usedHelper = () => 'hello';

// NOT STALE: this export IS unused, so the suppression is active
// fallow-ignore-next-line unused-export
export const unusedHelper = () => 'world';

// NOT STALE: this export is only used by another same-file export
// fallow-ignore-next-line unused-export
export const localOnlyHelper = 7;

export const reachableLocalConsumer = localOnlyHelper + 1;

// STALE: blanket suppression on a line with no issues
// fallow-ignore-next-line
export const anotherUsedExport = 42;
