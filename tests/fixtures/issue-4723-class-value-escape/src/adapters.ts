export class CallArgument {
  static async load(): Promise<string> {
    return "x";
  }
  static async save(data: string): Promise<void> {
    void data;
  }
  instanceOnly(): void {}
}

export class PropertyValue {
  static load(): string {
    return "x";
  }
  instanceOnly(): void {}
}

export class AliasArgument {
  static load(): string {
    return "x";
  }
  instanceOnly(): void {}
}

export class DirectRead {
  static used(): void {}
  static dead(): void {}
}

export class PrototypeRead {
  get handlesUpgrades(): boolean {
    return true;
  }
  static upgradesTo: string[] = [];
  other(): void {}
}

export class HolderRead {
  get handlesUpgrades(): boolean {
    return true;
  }
  other(): void {}
}

export class KeyedRead {
  keyed(): void {}
  missed(): void {}
}
