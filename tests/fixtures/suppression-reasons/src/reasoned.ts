// fallow-ignore-next-line unused-export -- public compatibility export
export const reasonedUnused = 'kept';

// fallow-ignore-next-line unused-export
export const missingReasonUnused = 'missing reason';

/** @expected-unused -- package surface */
export const expectedUnusedWithReason = 'documented';

/** @expected-unused */
export const expectedUnusedMissingReason = 'missing jsdoc reason';

export const used = 'used';
