export const usedInProd = (): string => "prod";

export const testOnly = (): string => "test";

export const testOnlyFar = (): string => "far";

export const storyOnly = (): string => "story";

export const fixtureOnly = (): string => "fixture";

export const helperOnly = (): string => "helper";

export const unusedFileOnly = (): string => "orphan";

/** @internal */
export const internalTestOnly = (): string => "internal";

/** @expected-unused kept for a later release */
export const expectedUnusedTestOnly = (): string => "expected";

export type TestOnlyShape = { id: string };
