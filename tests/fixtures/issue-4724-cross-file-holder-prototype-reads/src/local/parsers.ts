export class StrictParser {
  get strict(): boolean {
    return true;
  }

  unusedStrictHelper(): void {}
}

export class LooseParser {
  get strict(): boolean {
    return false;
  }
}

export const parsers = { strict: StrictParser, loose: LooseParser };

export const strictness = (name: "strict" | "loose"): boolean =>
  parsers[name].prototype.strict;
