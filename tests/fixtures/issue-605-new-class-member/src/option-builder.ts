// Fluent-chain-rooted-at-constructor shape (graphql-markdown#2949):
// `new OptionBuilder().addDefault(...).addFromCli(...).build()`.
export class OptionBuilder<T> {
  private value: T | undefined;

  addDefault(v: T, _key: string): this {
    return this.setIfProvided(v);
  }

  addFromConfig(v: T | undefined, _key: string): this {
    return this.setIfProvided(v);
  }

  addFromCli(v: T | undefined, _key: string): this {
    return this.setIfProvided(v);
  }

  build(): T | undefined {
    return this.value;
  }

  // Self-returning setter that is never called: must STILL be reported.
  addUnused(v: T): this {
    return this.setIfProvided(v);
  }

  private setIfProvided(v: T | undefined): this {
    this.value = v;
    return this;
  }

  // NOT self-returning (returns the resolved value). A chain that passes
  // through this method must NOT credit downstream members, mirroring the
  // #387 `.build().toString()` safety check.
  peek(): T | undefined {
    return this.value;
  }

  // Reached only as a downstream member after the non-self-returning `peek()`.
  // Must STILL be reported as unused: `new OptionBuilder().peek().afterPeek()`
  // leaves the OptionBuilder type at `peek()`.
  afterPeek(): void {}
}
