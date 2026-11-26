export class TestContext {
  pathUsed(): void {}

  destructuredUsed(): void {}

  nestedUsed(): void {}

  contextUnused(): void {}

  collisionOnly(): void {}

  defaultOnly(): void {}
}

export const newService = (name: string) => {
  const ctx = new TestContext();
  return { sut: name, ctx, nested: { inner: ctx } };
};
