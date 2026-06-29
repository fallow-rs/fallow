// `toString` is implicitly invoked when an instance is coerced to a string;
// each class below is coerced in a different position. `dead*` members are
// genuine dead members on the SAME classes and must keep reporting (the
// non-vacuous control). `NotCoerced.toString` is constructed but never coerced,
// so it must STILL report (scope is tight to coercion positions only).
export class Money {
  constructor(amount) {
    this.amount = amount;
  }
  toString() {
    return `$${this.amount}`;
  }
  deadMoney() {
    return 1;
  }
}

export class Label {
  toString() {
    return "label";
  }
  deadLabel() {
    return 2;
  }
}

export class Tag {
  toString() {
    return "tag";
  }
  deadTag() {
    return 3;
  }
}

export class NotCoerced {
  toString() {
    return "never coerced";
  }
}
