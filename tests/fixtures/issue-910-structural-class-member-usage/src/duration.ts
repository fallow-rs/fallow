export interface DurationI {
  toMs(): number;
  toSec(): number;
}

export class DurationMS {
  private readonly value: number;

  constructor(value: number) {
    this.value = value;
  }

  toMs(): number {
    return this.value;
  }

  toSec(): number {
    return this.value / 1000;
  }

  unused(): string {
    return 'unused';
  }
}
