// Each dep class carries one consumed member and one genuinely-dead control
// member. The control members must STAY flagged, proving the fix credits
// precisely per class and never blanket-credits.

export class DepPrivParam {
  privParam(): void {}
  deadOnPrivParam(): void {}
}

export class DepPubField {
  pubField(): void {}
  deadOnPubField(): void {}
}

export class DepHashA {
  hashA(): void {}
  deadOnHashA(): void {}
}

export class DepHashB {
  hashB(): void {}
  deadOnHashB(): void {}
}
