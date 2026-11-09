export class Day {
  #flag = false;
  viaIndex(): number {
    return 1;
  }
  viaThisIndex(): number {
    return 2;
  }
  viaCallback(): boolean {
    return this.#flag;
  }
  viaLoop(): void {
    this.#flag = false;
  }
  viaGetter(): number {
    return 3;
  }
  unused(): number {
    return 4;
  }
}
