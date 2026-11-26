export class PartnerFactory {
  private constructor(private value: { id: string }) {}

  static create(id: string) {
    return PartnerFactory.from(id).build();
  }

  static from(id: string) {
    return new PartnerFactory({ id }).sharedBy().sharedWith();
  }

  static async fromAsync(id: string) {
    return new PartnerFactory({ id }).sharedBy();
  }

  static fromFlag(id: string, flag: boolean) {
    if (flag) {
      return undefined;
    }
    return new PartnerFactory({ id }).sharedBy();
  }

  sharedBy() {
    return this;
  }

  sharedWith() {
    return this;
  }

  chained() {
    return this;
  }

  build() {
    return { ...this.value };
  }

  asyncOnly(): void {}

  flagOnly(): void {}

  fluentUnused(): number {
    return 1;
  }
}
