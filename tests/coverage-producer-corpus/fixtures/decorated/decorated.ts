declare const audit: MethodDecorator;

class Ledger {
  @audit
  balance(credit: number, debit: number): number {
    if (credit > debit) {
      return credit - debit;
    }
    return debit - credit;
  }
}

export { Ledger };
