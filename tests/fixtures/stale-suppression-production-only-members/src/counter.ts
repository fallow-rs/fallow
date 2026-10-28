export class Counter {
  // fallow-ignore-next-line unused-class-member -- called by the test harness only
  reset(): void {}
  increment(): void {}
  // fallow-ignore-next-line unused-class-member -- used by index and a test
  read(): number {
    return 0;
  }
}
