export class TestContext {
  blockWrapperUsed(): void {}

  conciseWrapperUsed(): void {}

  chainedWrapperUsed(): void {}

  pathWrapperUsed(): void {}

  contextUnused(): void {}

  shadowedOnly(): void {}

  repeatedUsed(): void {}

  repeatedDirectUsed(): void {}

  mixedOnly(): void {}

  shadowWrapperOnly(): void {}

  reassignedWrapperOnly(): void {}

  importOnly(): void {}
}

export const newService = (name: string) => {
  const ctx = new TestContext();
  return { sut: name, ctx };
};
