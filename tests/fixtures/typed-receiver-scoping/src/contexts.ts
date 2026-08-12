export class FirstContext {
  firstUsed(): void {}
  firstDead(): void {}
}

export class SecondContext {
  secondUsed(): void {}
  secondDead(): void {}
}

export class AliasContext {
  aliasUsed(): void {}
  pickedOnly(): void {}
  aliasDead(): void {}
}

export class NestedContext {
  aliasUsed(): void {}
  nestedDead(): void {}
}
