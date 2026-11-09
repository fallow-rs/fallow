export class Sheet {
  viaCallback = '';

  viaLoop(): number {
    return 1;
  }

  viaMap(): number {
    return 2;
  }

  viaDirectLoop(): number {
    return 3;
  }

  viaIndex(): number {
    return 4;
  }

  unused(): number {
    return 5;
  }
}

export class Single {
  viaLoop(): number {
    return 1;
  }
}

export const make = (): Sheet[] => [new Sheet()];

export function makeSingle(): Single {
  return new Single();
}
