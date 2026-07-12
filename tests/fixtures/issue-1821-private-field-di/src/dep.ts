export class DepInlineNew {
  inlineM(): void {}
  deadOnInlineNew(): void {}
}

export class DepCtorNew {
  ctorNewM(): void {}
  deadOnCtorNew(): void {}
}

export interface IDep {
  ifaceM(): void;
}

export class DepIface implements IDep {
  ifaceM(): void {}
  deadOnDepIface(): void {}
}
